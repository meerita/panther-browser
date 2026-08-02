// @file engines/purr/engine/src/html-tokenizer.rs
// @description Defines the minimal iterative HTML tokenizer for the M2 pipeline.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Minimal iterative HTML tokenizer.
//!
//! The tokenizer turns source bytes into a fixed `HtmlToken` set that the tree
//! builder consumes. It is the first untrusted-input parser, so it enforces its
//! security limits from the start: it runs iteratively with an explicit state
//! machine and no recursion, applies a per-token size or count cap with checked
//! arithmetic before it grows any buffer, and aborts with a typed error on an
//! over-limit token instead of a silent truncation. Malformed input where the
//! specification recovers raises a recoverable parse-error signal and keeps
//! going, never a panic.
//!
//! The input unit is a code point, not a Rust `char`. The source is decoded to
//! code points once at entry, and the public token type carries `CodePoint` so
//! the interface stays independent of the internal representation and of a later
//! non-UTF-8 decoder. `<title>` content is tokenized as RCDATA.

// The tree builder (a later phase) is the first non-test consumer of the token
// stream. This phase adds the tokenizer and exercises it through the unit tests
// below, so the public items are otherwise unused in a non-test build.
#![allow(dead_code)]

use std::collections::VecDeque;

/// Upper bound for the byte length of a tag name.
pub const MAX_TAG_NAME_LEN: usize = 1024;
/// Upper bound for the byte length of an attribute name.
pub const MAX_ATTRIBUTE_NAME_LEN: usize = 1024;
/// Upper bound for the byte length of an attribute value.
pub const MAX_ATTRIBUTE_VALUE_LEN: usize = 65_536;
/// Upper bound for the number of attributes on one tag.
pub const MAX_ATTRIBUTES_PER_TAG: usize = 256;
/// Upper bound for the byte length of a comment.
pub const MAX_COMMENT_LEN: usize = 65_536;
/// Upper bound for the byte length of a doctype.
pub const MAX_DOCTYPE_LEN: usize = 1024;

const REPLACEMENT: char = '\u{FFFD}';

/// One decoded code point from the source.
///
/// A code point is a Unicode scalar value produced by decoding the source. The
/// wrapper keeps the token interface distinct from a Rust `char`, so the input
/// unit stays a code point and a later non-UTF-8 decoder does not change the
/// token type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodePoint(char);

impl CodePoint {
    pub fn new(value: char) -> Self {
        Self(value)
    }

    pub fn get(self) -> char {
        self.0
    }
}

/// One tag attribute as an ordered name/value pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

/// A start tag with its ordered attributes and self-closing flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartTag {
    pub name: String,
    pub attributes: Vec<Attribute>,
    pub self_closing: bool,
}

/// An end tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndTag {
    pub name: String,
}

/// A doctype declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doctype {
    pub name: Option<String>,
}

/// One token the tokenizer emits.
///
/// The set is fixed for the M2 subset: a doctype, a start tag, an end tag, a
/// single character code point, a comment, and end-of-file. The tree builder
/// coalesces adjacent character tokens into document text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlToken {
    Doctype(Doctype),
    StartTag(StartTag),
    EndTag(EndTag),
    Character(CodePoint),
    Comment(String),
    EndOfFile,
}

/// Failure the tokenizer reports when an over-limit token aborts tokenization.
///
/// These are fatal: the tokenizer owns them and aborts on the first breach. A
/// later phase translates them into the store's document error at the point a
/// parse failure surfaces to the seam. Each message is a static, factual,
/// non-secret string. Recoverable parse errors do not use this type; they raise
/// the parse-error signal and tokenization continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenizerError {
    #[error("a tag name exceeds the maximum length")]
    TagNameTooLong,
    #[error("an attribute name exceeds the maximum length")]
    AttributeNameTooLong,
    #[error("an attribute value exceeds the maximum length")]
    AttributeValueTooLong,
    #[error("a tag has more than the maximum number of attributes")]
    TooManyAttributes,
    #[error("a comment exceeds the maximum length")]
    CommentTooLong,
    #[error("a doctype exceeds the maximum length")]
    DoctypeTooLong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentEndDash,
    CommentEnd,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    BogusDoctype,
    Rcdata,
    RcdataLessThanSign,
    RcdataEndTagOpen,
    RcdataEndTagName,
}

struct TagBuilder {
    is_end: bool,
    name: String,
    attributes: Vec<Attribute>,
    self_closing: bool,
    attr_name: String,
    attr_value: String,
    in_attr: bool,
}

impl TagBuilder {
    fn new(is_end: bool) -> Self {
        Self {
            is_end,
            name: String::new(),
            attributes: Vec::new(),
            self_closing: false,
            attr_name: String::new(),
            attr_value: String::new(),
            in_attr: false,
        }
    }

    /// Commits the in-progress attribute, dropping a duplicate name.
    fn commit_attribute(&mut self) {
        if self.in_attr && !self.attr_name.is_empty() {
            let duplicate = self
                .attributes
                .iter()
                .any(|attribute| attribute.name == self.attr_name);
            if !duplicate {
                self.attributes.push(Attribute {
                    name: std::mem::take(&mut self.attr_name),
                    value: std::mem::take(&mut self.attr_value),
                });
            }
        }
        self.in_attr = false;
        self.attr_name.clear();
        self.attr_value.clear();
    }

    /// Starts a new attribute after the attribute-count cap check.
    fn begin_attribute(&mut self) -> Result<(), TokenizerError> {
        self.commit_attribute();
        if self.attributes.len() >= MAX_ATTRIBUTES_PER_TAG {
            return Err(TokenizerError::TooManyAttributes);
        }
        self.in_attr = true;
        Ok(())
    }
}

/// Iterative HTML tokenizer.
///
/// The tokenizer decodes the source once, then yields one token per
/// `next_token` call. It never recurses. A hard per-token cap aborts with a
/// `TokenizerError`; a recoverable condition increments the parse-error count
/// and tokenization continues.
pub struct Tokenizer {
    input: Vec<char>,
    position: usize,
    state: State,
    pending: VecDeque<HtmlToken>,
    tag: Option<TagBuilder>,
    comment: String,
    doctype_name: Option<String>,
    doctype_length: usize,
    rcdata_name: String,
    temporary: String,
    parse_errors: usize,
    finished: bool,
}

impl Tokenizer {
    pub fn new(source: &[u8]) -> Self {
        Self {
            input: decode(source),
            position: 0,
            state: State::Data,
            pending: VecDeque::new(),
            tag: None,
            comment: String::new(),
            doctype_name: None,
            doctype_length: 0,
            rcdata_name: String::new(),
            temporary: String::new(),
            parse_errors: 0,
            finished: false,
        }
    }

    /// The number of recoverable parse errors seen so far.
    pub fn parse_error_count(&self) -> usize {
        self.parse_errors
    }

    /// Returns the next token, or a `TokenizerError` when an over-limit token
    /// aborts tokenization.
    ///
    /// After end-of-file every further call returns `EndOfFile`.
    pub fn next_token(&mut self) -> Result<HtmlToken, TokenizerError> {
        loop {
            if let Some(token) = self.pending.pop_front() {
                return Ok(token);
            }
            if self.finished {
                return Ok(HtmlToken::EndOfFile);
            }
            self.step()?;
        }
    }

    fn step(&mut self) -> Result<(), TokenizerError> {
        let Some(current) = self.consume() else {
            self.handle_end_of_file();
            return Ok(());
        };

        match self.state {
            State::Data => self.on_data(current),
            State::TagOpen => self.on_tag_open(current),
            State::EndTagOpen => self.on_end_tag_open(current),
            State::TagName => self.on_tag_name(current),
            State::BeforeAttributeName => self.on_before_attribute_name(current),
            State::AttributeName => self.on_attribute_name(current),
            State::AfterAttributeName => self.on_after_attribute_name(current),
            State::BeforeAttributeValue => self.on_before_attribute_value(current),
            State::AttributeValueDoubleQuoted => self.on_attribute_value(current, '"'),
            State::AttributeValueSingleQuoted => self.on_attribute_value(current, '\''),
            State::AttributeValueUnquoted => self.on_attribute_value_unquoted(current),
            State::AfterAttributeValueQuoted => self.on_after_attribute_value_quoted(current),
            State::SelfClosingStartTag => self.on_self_closing_start_tag(current),
            State::BogusComment => self.on_bogus_comment(current),
            State::CommentStart => self.on_comment_start(current),
            State::CommentStartDash => self.on_comment_start_dash(current),
            State::Comment => self.on_comment(current),
            State::CommentEndDash => self.on_comment_end_dash(current),
            State::CommentEnd => self.on_comment_end(current),
            State::Doctype => self.on_doctype(current),
            State::BeforeDoctypeName => self.on_before_doctype_name(current),
            State::DoctypeName => self.on_doctype_name(current),
            State::AfterDoctypeName => self.on_after_doctype_name(current),
            State::BogusDoctype => self.on_bogus_doctype(current),
            State::Rcdata => self.on_rcdata(current),
            State::RcdataLessThanSign => self.on_rcdata_less_than_sign(current),
            State::RcdataEndTagOpen => self.on_rcdata_end_tag_open(current),
            State::RcdataEndTagName => self.on_rcdata_end_tag_name(current),
        }
    }

    fn on_data(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '&' => {
                let decoded = self.consume_character_reference().unwrap_or('&');
                self.emit_character(decoded);
            }
            '<' => self.state = State::TagOpen,
            '\0' => {
                self.parse_error();
                self.emit_character(REPLACEMENT);
            }
            other => self.emit_character(other),
        }
        Ok(())
    }

    fn on_tag_open(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '!' => self.open_markup_declaration(),
            '/' => self.state = State::EndTagOpen,
            letter if letter.is_ascii_alphabetic() => {
                self.tag = Some(TagBuilder::new(false));
                self.reconsume();
                self.state = State::TagName;
            }
            '?' => {
                self.parse_error();
                self.comment.clear();
                self.reconsume();
                self.state = State::BogusComment;
            }
            _ => {
                self.parse_error();
                self.emit_character('<');
                self.reconsume();
                self.state = State::Data;
            }
        }
        Ok(())
    }

    fn on_end_tag_open(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            letter if letter.is_ascii_alphabetic() => {
                self.tag = Some(TagBuilder::new(true));
                self.reconsume();
                self.state = State::TagName;
            }
            '>' => {
                self.parse_error();
                self.state = State::Data;
            }
            _ => {
                self.parse_error();
                self.comment.clear();
                self.reconsume();
                self.state = State::BogusComment;
            }
        }
        Ok(())
    }

    fn on_tag_name(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => self.state = State::BeforeAttributeName,
            '/' => self.state = State::SelfClosingStartTag,
            '>' => self.emit_current_tag(),
            '\0' => {
                self.parse_error();
                self.append_tag_name(REPLACEMENT)?;
            }
            other => self.append_tag_name(other.to_ascii_lowercase())?,
        }
        Ok(())
    }

    fn on_before_attribute_name(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => {}
            '/' | '>' => {
                self.reconsume();
                self.state = State::AfterAttributeName;
            }
            '=' => {
                self.parse_error();
                self.begin_attribute()?;
                self.append_attribute_name('=')?;
                self.state = State::AttributeName;
            }
            _ => {
                self.begin_attribute()?;
                self.reconsume();
                self.state = State::AttributeName;
            }
        }
        Ok(())
    }

    fn on_attribute_name(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => {
                self.reconsume();
                self.state = State::AfterAttributeName;
            }
            '/' | '>' => {
                self.reconsume();
                self.state = State::AfterAttributeName;
            }
            '=' => self.state = State::BeforeAttributeValue,
            '\0' => {
                self.parse_error();
                self.append_attribute_name(REPLACEMENT)?;
            }
            '"' | '\'' | '<' => {
                self.parse_error();
                self.append_attribute_name(current)?;
            }
            other => self.append_attribute_name(other.to_ascii_lowercase())?,
        }
        Ok(())
    }

    fn on_after_attribute_name(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => {}
            '/' => self.state = State::SelfClosingStartTag,
            '=' => self.state = State::BeforeAttributeValue,
            '>' => self.emit_current_tag(),
            _ => {
                self.begin_attribute()?;
                self.reconsume();
                self.state = State::AttributeName;
            }
        }
        Ok(())
    }

    fn on_before_attribute_value(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => {}
            '"' => self.state = State::AttributeValueDoubleQuoted,
            '\'' => self.state = State::AttributeValueSingleQuoted,
            '>' => {
                self.parse_error();
                self.emit_current_tag();
            }
            _ => {
                self.reconsume();
                self.state = State::AttributeValueUnquoted;
            }
        }
        Ok(())
    }

    fn on_attribute_value(&mut self, current: char, closing: char) -> Result<(), TokenizerError> {
        if current == closing {
            self.state = State::AfterAttributeValueQuoted;
            return Ok(());
        }
        match current {
            '&' => {
                let decoded = self.consume_character_reference().unwrap_or('&');
                self.append_attribute_value(decoded)?;
            }
            '\0' => {
                self.parse_error();
                self.append_attribute_value(REPLACEMENT)?;
            }
            other => self.append_attribute_value(other)?,
        }
        Ok(())
    }

    fn on_attribute_value_unquoted(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => self.state = State::BeforeAttributeName,
            '&' => {
                let decoded = self.consume_character_reference().unwrap_or('&');
                self.append_attribute_value(decoded)?;
            }
            '>' => self.emit_current_tag(),
            '\0' => {
                self.parse_error();
                self.append_attribute_value(REPLACEMENT)?;
            }
            '"' | '\'' | '<' | '=' | '`' => {
                self.parse_error();
                self.append_attribute_value(current)?;
            }
            other => self.append_attribute_value(other)?,
        }
        Ok(())
    }

    fn on_after_attribute_value_quoted(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => self.state = State::BeforeAttributeName,
            '/' => self.state = State::SelfClosingStartTag,
            '>' => self.emit_current_tag(),
            _ => {
                self.parse_error();
                self.reconsume();
                self.state = State::BeforeAttributeName;
            }
        }
        Ok(())
    }

    fn on_self_closing_start_tag(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '>' => {
                if let Some(tag) = self.tag.as_mut() {
                    tag.self_closing = true;
                }
                self.emit_current_tag();
            }
            _ => {
                self.parse_error();
                self.reconsume();
                self.state = State::BeforeAttributeName;
            }
        }
        Ok(())
    }

    fn on_bogus_comment(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '>' => self.emit_comment(),
            '\0' => self.append_comment(REPLACEMENT)?,
            other => self.append_comment(other)?,
        }
        Ok(())
    }

    fn on_comment_start(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '-' => self.state = State::CommentStartDash,
            '>' => {
                self.parse_error();
                self.emit_comment();
            }
            _ => {
                self.reconsume();
                self.state = State::Comment;
            }
        }
        Ok(())
    }

    fn on_comment_start_dash(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '-' => self.state = State::CommentEnd,
            '>' => {
                self.parse_error();
                self.emit_comment();
            }
            _ => {
                self.append_comment('-')?;
                self.reconsume();
                self.state = State::Comment;
            }
        }
        Ok(())
    }

    fn on_comment(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '-' => self.state = State::CommentEndDash,
            '\0' => {
                self.parse_error();
                self.append_comment(REPLACEMENT)?;
            }
            other => self.append_comment(other)?,
        }
        Ok(())
    }

    fn on_comment_end_dash(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '-' => self.state = State::CommentEnd,
            _ => {
                self.append_comment('-')?;
                self.reconsume();
                self.state = State::Comment;
            }
        }
        Ok(())
    }

    fn on_comment_end(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '>' => self.emit_comment(),
            '-' => self.append_comment('-')?,
            _ => {
                self.append_comment('-')?;
                self.append_comment('-')?;
                self.reconsume();
                self.state = State::Comment;
            }
        }
        Ok(())
    }

    fn on_doctype(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => self.state = State::BeforeDoctypeName,
            _ => {
                self.parse_error();
                self.reconsume();
                self.state = State::BeforeDoctypeName;
            }
        }
        Ok(())
    }

    fn on_before_doctype_name(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => {}
            '>' => {
                self.parse_error();
                self.start_doctype(false);
                self.emit_doctype();
            }
            '\0' => {
                self.parse_error();
                self.start_doctype(true);
                self.append_doctype_name(REPLACEMENT)?;
                self.state = State::DoctypeName;
            }
            other => {
                self.start_doctype(true);
                self.append_doctype_name(other.to_ascii_lowercase())?;
                self.state = State::DoctypeName;
            }
        }
        Ok(())
    }

    fn on_doctype_name(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => self.state = State::AfterDoctypeName,
            '>' => self.emit_doctype(),
            '\0' => {
                self.parse_error();
                self.append_doctype_name(REPLACEMENT)?;
            }
            other => self.append_doctype_name(other.to_ascii_lowercase())?,
        }
        Ok(())
    }

    fn on_after_doctype_name(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            whitespace if is_whitespace(whitespace) => {}
            '>' => self.emit_doctype(),
            _ => {
                self.parse_error();
                self.reconsume();
                self.state = State::BogusDoctype;
            }
        }
        Ok(())
    }

    fn on_bogus_doctype(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '>' => self.emit_doctype(),
            other => self.count_doctype(other)?,
        }
        Ok(())
    }

    fn on_rcdata(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '&' => {
                let decoded = self.consume_character_reference().unwrap_or('&');
                self.emit_character(decoded);
            }
            '<' => {
                self.temporary.clear();
                self.state = State::RcdataLessThanSign;
            }
            '\0' => {
                self.parse_error();
                self.emit_character(REPLACEMENT);
            }
            other => self.emit_character(other),
        }
        Ok(())
    }

    fn on_rcdata_less_than_sign(&mut self, current: char) -> Result<(), TokenizerError> {
        match current {
            '/' => {
                self.temporary.clear();
                self.state = State::RcdataEndTagOpen;
            }
            _ => {
                self.emit_character('<');
                self.reconsume();
                self.state = State::Rcdata;
            }
        }
        Ok(())
    }

    fn on_rcdata_end_tag_open(&mut self, current: char) -> Result<(), TokenizerError> {
        if current.is_ascii_alphabetic() {
            self.tag = Some(TagBuilder::new(true));
            self.reconsume();
            self.state = State::RcdataEndTagName;
        } else {
            self.emit_character('<');
            self.emit_character('/');
            self.reconsume();
            self.state = State::Rcdata;
        }
        Ok(())
    }

    fn on_rcdata_end_tag_name(&mut self, current: char) -> Result<(), TokenizerError> {
        if current.is_ascii_alphabetic() {
            self.temporary.push(current);
            self.append_tag_name(current.to_ascii_lowercase())?;
            return Ok(());
        }

        if self.end_tag_matches_rcdata() {
            match current {
                whitespace if is_whitespace(whitespace) => {
                    self.state = State::BeforeAttributeName;
                    return Ok(());
                }
                '/' => {
                    self.state = State::SelfClosingStartTag;
                    return Ok(());
                }
                '>' => {
                    self.emit_current_tag();
                    return Ok(());
                }
                _ => {}
            }
        }

        self.tag = None;
        self.emit_character('<');
        self.emit_character('/');
        let buffered: Vec<char> = self.temporary.chars().collect();
        for character in buffered {
            self.emit_character(character);
        }
        self.reconsume();
        self.state = State::Rcdata;
        Ok(())
    }

    fn handle_end_of_file(&mut self) {
        match self.state {
            State::BogusComment
            | State::CommentStart
            | State::CommentStartDash
            | State::Comment
            | State::CommentEndDash
            | State::CommentEnd => {
                self.parse_error();
                self.emit_comment();
            }
            State::Doctype
            | State::BeforeDoctypeName
            | State::DoctypeName
            | State::AfterDoctypeName
            | State::BogusDoctype => {
                self.parse_error();
                self.emit_doctype();
            }
            State::Data | State::Rcdata => {}
            _ => self.parse_error(),
        }
        self.finished = true;
    }

    fn open_markup_declaration(&mut self) {
        if self.matches_ahead("--") {
            self.position += 2;
            self.comment.clear();
            self.state = State::CommentStart;
        } else if self.matches_ahead_ignore_case("doctype") {
            self.position += 7;
            self.state = State::Doctype;
        } else {
            self.parse_error();
            self.comment.clear();
            self.state = State::BogusComment;
        }
    }

    fn emit_current_tag(&mut self) {
        let Some(mut tag) = self.tag.take() else {
            self.state = State::Data;
            return;
        };
        tag.commit_attribute();

        if tag.is_end {
            self.pending
                .push_back(HtmlToken::EndTag(EndTag { name: tag.name }));
            self.state = State::Data;
            return;
        }

        let enters_rcdata = !tag.self_closing && is_rcdata_element(&tag.name);
        if enters_rcdata {
            self.rcdata_name = tag.name.clone();
        }
        self.pending.push_back(HtmlToken::StartTag(StartTag {
            name: tag.name,
            attributes: tag.attributes,
            self_closing: tag.self_closing,
        }));
        self.state = if enters_rcdata {
            State::Rcdata
        } else {
            State::Data
        };
    }

    fn emit_comment(&mut self) {
        self.pending
            .push_back(HtmlToken::Comment(std::mem::take(&mut self.comment)));
        self.state = State::Data;
    }

    fn emit_doctype(&mut self) {
        self.pending.push_back(HtmlToken::Doctype(Doctype {
            name: self.doctype_name.take(),
        }));
        self.doctype_length = 0;
        self.state = State::Data;
    }

    fn emit_character(&mut self, value: char) {
        self.pending
            .push_back(HtmlToken::Character(CodePoint::new(value)));
    }

    fn end_tag_matches_rcdata(&self) -> bool {
        self.tag
            .as_ref()
            .is_some_and(|tag| tag.name == self.rcdata_name)
    }

    fn start_doctype(&mut self, named: bool) {
        self.doctype_name = if named { Some(String::new()) } else { None };
        self.doctype_length = 0;
    }

    fn append_tag_name(&mut self, value: char) -> Result<(), TokenizerError> {
        let Some(tag) = self.tag.as_mut() else {
            return Ok(());
        };
        push_capped(
            &mut tag.name,
            value,
            MAX_TAG_NAME_LEN,
            TokenizerError::TagNameTooLong,
        )
    }

    fn append_attribute_name(&mut self, value: char) -> Result<(), TokenizerError> {
        let Some(tag) = self.tag.as_mut() else {
            return Ok(());
        };
        push_capped(
            &mut tag.attr_name,
            value,
            MAX_ATTRIBUTE_NAME_LEN,
            TokenizerError::AttributeNameTooLong,
        )
    }

    fn append_attribute_value(&mut self, value: char) -> Result<(), TokenizerError> {
        let Some(tag) = self.tag.as_mut() else {
            return Ok(());
        };
        push_capped(
            &mut tag.attr_value,
            value,
            MAX_ATTRIBUTE_VALUE_LEN,
            TokenizerError::AttributeValueTooLong,
        )
    }

    fn append_comment(&mut self, value: char) -> Result<(), TokenizerError> {
        push_capped(
            &mut self.comment,
            value,
            MAX_COMMENT_LEN,
            TokenizerError::CommentTooLong,
        )
    }

    fn append_doctype_name(&mut self, value: char) -> Result<(), TokenizerError> {
        self.count_doctype(value)?;
        if let Some(name) = self.doctype_name.as_mut() {
            name.push(value);
        }
        Ok(())
    }

    fn count_doctype(&mut self, value: char) -> Result<(), TokenizerError> {
        let projected = self
            .doctype_length
            .checked_add(value.len_utf8())
            .ok_or(TokenizerError::DoctypeTooLong)?;
        if projected > MAX_DOCTYPE_LEN {
            return Err(TokenizerError::DoctypeTooLong);
        }
        self.doctype_length = projected;
        Ok(())
    }

    fn begin_attribute(&mut self) -> Result<(), TokenizerError> {
        match self.tag.as_mut() {
            Some(tag) => tag.begin_attribute(),
            None => Ok(()),
        }
    }

    fn parse_error(&mut self) {
        self.parse_errors = self.parse_errors.saturating_add(1);
    }

    /// Consumes and returns the current code point, advancing the cursor.
    ///
    /// Returns `None` at end-of-file without advancing.
    fn consume(&mut self) -> Option<char> {
        let current = self.input.get(self.position).copied()?;
        self.position += 1;
        Some(current)
    }

    /// Steps the cursor back one code point so the next `consume` returns it
    /// again.
    ///
    /// Only a state that just consumed a code point reconsumes, so the cursor is
    /// never before the start.
    fn reconsume(&mut self) {
        self.position = self.position.saturating_sub(1);
    }

    fn matches_ahead(&self, needle: &str) -> bool {
        let mut cursor = self.position;
        for expected in needle.chars() {
            match self.input.get(cursor) {
                Some(actual) if *actual == expected => cursor += 1,
                _ => return false,
            }
        }
        true
    }

    fn matches_ahead_ignore_case(&self, needle: &str) -> bool {
        let mut cursor = self.position;
        for expected in needle.chars() {
            match self.input.get(cursor) {
                Some(actual) if actual.eq_ignore_ascii_case(&expected) => cursor += 1,
                _ => return false,
            }
        }
        true
    }

    /// Decodes a character reference positioned just after the `&`.
    ///
    /// Returns the decoded scalar value and advances the cursor on success, or
    /// `None` without advancing when the reference does not match the supported
    /// subset. The caller then treats the `&` as literal text and reprocesses
    /// the following code points.
    fn consume_character_reference(&mut self) -> Option<char> {
        match self.input.get(self.position).copied()? {
            '#' => self.consume_numeric_reference(),
            _ => self.consume_named_reference(),
        }
    }

    fn consume_named_reference(&mut self) -> Option<char> {
        const NAMED: [(&str, char); 4] =
            [("amp;", '&'), ("lt;", '<'), ("gt;", '>'), ("quot;", '"')];
        for (name, value) in NAMED {
            if self.matches_ahead(name) {
                self.position += name.chars().count();
                return Some(value);
            }
        }
        None
    }

    fn consume_numeric_reference(&mut self) -> Option<char> {
        let mut cursor = self.position + 1;
        let hex = matches!(self.input.get(cursor), Some('x' | 'X'));
        if hex {
            cursor += 1;
        }

        let radix = if hex { 16 } else { 10 };
        let digits_start = cursor;
        let mut value: u32 = 0;
        while let Some(digit) = self.input.get(cursor).and_then(|c| c.to_digit(radix)) {
            value = value.saturating_mul(radix).saturating_add(digit);
            cursor += 1;
        }

        if cursor == digits_start {
            return None;
        }
        if matches!(self.input.get(cursor), Some(';')) {
            cursor += 1;
        }

        self.position = cursor;
        Some(scalar_value(value))
    }
}

/// Appends a code point after a checked byte-length cap.
///
/// Fails closed with `error` when the projected byte length overflows or exceeds
/// `cap`, before it grows the buffer.
fn push_capped(
    buffer: &mut String,
    value: char,
    cap: usize,
    error: TokenizerError,
) -> Result<(), TokenizerError> {
    let projected = buffer.len().checked_add(value.len_utf8()).ok_or(error)?;
    if projected > cap {
        return Err(error);
    }
    buffer.push(value);
    Ok(())
}

/// Maps a numeric reference value to a safe scalar value.
///
/// A null, out-of-range, or surrogate value maps to the replacement character,
/// so a hostile numeric reference cannot inject a forbidden code point.
fn scalar_value(value: u32) -> char {
    if value == 0 || value > 0x0010_FFFF || (0xD800..=0xDFFF).contains(&value) {
        return REPLACEMENT;
    }
    char::from_u32(value).unwrap_or(REPLACEMENT)
}

fn is_whitespace(value: char) -> bool {
    matches!(value, ' ' | '\t' | '\n' | '\u{000C}')
}

fn is_rcdata_element(name: &str) -> bool {
    matches!(name, "title" | "textarea")
}

/// Decodes source bytes to code points and normalizes newlines.
///
/// Invalid UTF-8 becomes the replacement character. A `\r\n` pair and a lone
/// `\r` both become a single `\n`, matching the input preprocessing the
/// tokenizer expects.
fn decode(source: &[u8]) -> Vec<char> {
    let text = String::from_utf8_lossy(source);
    let mut output = Vec::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            output.push('\n');
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(source: &str) -> Result<Vec<HtmlToken>, TokenizerError> {
        let mut tokenizer = Tokenizer::new(source.as_bytes());
        let mut tokens = Vec::new();
        loop {
            let token = tokenizer.next_token()?;
            let is_end = token == HtmlToken::EndOfFile;
            tokens.push(token);
            if is_end {
                return Ok(tokens);
            }
        }
    }

    fn start(name: &str) -> HtmlToken {
        HtmlToken::StartTag(StartTag {
            name: name.to_owned(),
            attributes: Vec::new(),
            self_closing: false,
        })
    }

    fn end(name: &str) -> HtmlToken {
        HtmlToken::EndTag(EndTag {
            name: name.to_owned(),
        })
    }

    fn character(value: char) -> HtmlToken {
        HtmlToken::Character(CodePoint::new(value))
    }

    #[test]
    fn tokenizes_a_well_formed_snippet() {
        let tokens = tokenize(
            "<!doctype html><html><head><title>t</title></head><body><p>hi</p></body></html>",
        )
        .expect("no cap is exceeded");

        assert_eq!(
            tokens,
            vec![
                HtmlToken::Doctype(Doctype {
                    name: Some("html".to_owned()),
                }),
                start("html"),
                start("head"),
                start("title"),
                character('t'),
                end("title"),
                end("head"),
                start("body"),
                start("p"),
                character('h'),
                character('i'),
                end("p"),
                end("body"),
                end("html"),
                HtmlToken::EndOfFile,
            ]
        );
    }

    #[test]
    fn parses_an_attribute_in_order() {
        let tokens = tokenize("<a href=\"x\">").expect("no cap is exceeded");

        assert_eq!(
            tokens.first(),
            Some(&HtmlToken::StartTag(StartTag {
                name: "a".to_owned(),
                attributes: vec![Attribute {
                    name: "href".to_owned(),
                    value: "x".to_owned(),
                }],
                self_closing: false,
            }))
        );
    }

    #[test]
    fn decodes_the_character_reference_subset() {
        let tokens = tokenize("&amp;&lt;&gt;&quot;&#65;&#x41;").expect("no cap is exceeded");

        assert_eq!(
            tokens,
            vec![
                character('&'),
                character('<'),
                character('>'),
                character('"'),
                character('A'),
                character('A'),
                HtmlToken::EndOfFile,
            ]
        );
    }

    #[test]
    fn an_unknown_reference_recovers_without_panic() {
        let tokens = tokenize("&nope;").expect("no cap is exceeded");

        assert_eq!(
            tokens,
            vec![
                character('&'),
                character('n'),
                character('o'),
                character('p'),
                character('e'),
                character(';'),
                HtmlToken::EndOfFile,
            ]
        );
    }

    #[test]
    fn title_content_is_rcdata_until_the_matching_end_tag() {
        let tokens = tokenize("<title>x<y</title>").expect("no cap is exceeded");

        assert_eq!(
            tokens,
            vec![
                start("title"),
                character('x'),
                character('<'),
                character('y'),
                end("title"),
                HtmlToken::EndOfFile,
            ]
        );
    }

    #[test]
    fn a_tag_name_at_the_cap_tokenizes() {
        let source = format!("<{}>", "a".repeat(MAX_TAG_NAME_LEN));
        let tokens = tokenize(&source).expect("the boundary length is accepted");

        assert_eq!(tokens.first(), Some(&start(&"a".repeat(MAX_TAG_NAME_LEN))));
    }

    #[test]
    fn an_over_limit_tag_name_aborts() {
        let source = format!("<{}>", "a".repeat(MAX_TAG_NAME_LEN + 1));
        assert_eq!(tokenize(&source), Err(TokenizerError::TagNameTooLong));
    }

    #[test]
    fn an_over_limit_attribute_name_aborts() {
        let source = format!("<a {}>", "n".repeat(MAX_ATTRIBUTE_NAME_LEN + 1));
        assert_eq!(tokenize(&source), Err(TokenizerError::AttributeNameTooLong));
    }

    #[test]
    fn an_over_limit_attribute_value_aborts() {
        let source = format!("<a x=\"{}\">", "v".repeat(MAX_ATTRIBUTE_VALUE_LEN + 1));
        assert_eq!(
            tokenize(&source),
            Err(TokenizerError::AttributeValueTooLong)
        );
    }

    #[test]
    fn too_many_attributes_abort() {
        let mut source = String::from("<a");
        for index in 0..=MAX_ATTRIBUTES_PER_TAG {
            source.push_str(&format!(" x{index}"));
        }
        source.push('>');
        assert_eq!(tokenize(&source), Err(TokenizerError::TooManyAttributes));
    }

    #[test]
    fn an_over_limit_comment_aborts() {
        let source = format!("<!--{}-->", "c".repeat(MAX_COMMENT_LEN + 1));
        assert_eq!(tokenize(&source), Err(TokenizerError::CommentTooLong));
    }

    #[test]
    fn an_over_limit_doctype_aborts() {
        let source = format!("<!doctype {}>", "d".repeat(MAX_DOCTYPE_LEN + 1));
        assert_eq!(tokenize(&source), Err(TokenizerError::DoctypeTooLong));
    }

    #[test]
    fn a_truncated_tag_ends_with_end_of_file() {
        let tokens = tokenize("<div class=\"x").expect("truncation is recoverable");

        assert_eq!(tokens.last(), Some(&HtmlToken::EndOfFile));
    }

    #[test]
    fn a_truncated_comment_emits_the_comment_then_end_of_file() {
        let tokens = tokenize("<!-- open").expect("truncation is recoverable");

        assert_eq!(
            tokens,
            vec![HtmlToken::Comment(" open".to_owned()), HtmlToken::EndOfFile,]
        );
    }
}
