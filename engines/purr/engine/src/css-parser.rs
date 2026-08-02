// @file engines/purr/engine/src/css-parser.rs
// @description Parses CSS tokens into immutable rule structures for the M2 cascade.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Minimal CSS parser.
//!
//! The parser turns the tokenizer output into immutable rule structures the
//! cascade reads: a stylesheet of rules, each rule a selector list and a
//! declaration block, each selector a compound of an optional type, zero or more
//! classes, and an optional id with a non-overflowing specificity, and each
//! declaration a property id, a specified value, an importance flag, a
//! source-order index, and a validity flag.
//!
//! The structures hold logical values only: no geometry and no GPU or native
//! type. The specified value stays the serialized value text, which the cascade
//! and computed-style phase interpret.
//!
//! The parser is bounded and recovers by dropping. An invalid declaration is
//! dropped and the valid declarations in the same rule survive; an invalid rule
//! is dropped and the following rules survive. A per-rule or per-sheet cap
//! rejects an over-limit rule while the previously committed structures are kept.
//! The two `margin` and `padding` shorthands expand into their longhands.

// The cascade (a later phase) is the first consumer of these structures. This
// phase builds and tests them, so some accessors are otherwise unused in a
// non-test build.
#![allow(dead_code)]

use crate::css_tokenizer::{CssToken, tokenize};

/// Upper bound for the byte length of one selector.
pub const MAX_SELECTOR_LEN: usize = 256;
/// Upper bound for the number of selectors in one selector list.
pub const MAX_SELECTORS_PER_LIST: usize = 64;
/// Upper bound for the number of source declarations in one rule.
pub const MAX_DECLARATIONS_PER_RULE: usize = 256;
/// Upper bound for the byte length of one specified value.
pub const MAX_VALUE_LEN: usize = 1024;
/// Upper bound for the number of rules in one stylesheet.
pub const MAX_RULES_PER_SHEET: usize = 4096;
/// Upper bound for the number of rules across the sheets of one document.
///
/// The per-document cap is applied when the cascade assembles the origin sheets
/// of one document in a later phase; a single sheet stays under the per-sheet
/// cap above.
pub const MAX_RULES_PER_DOCUMENT: usize = 8192;

/// Cascade origin of a stylesheet.
///
/// The user-agent origin carries the mandatory baseline sheet; the author origin
/// carries one embedded `<style>`. The origin orders the cascade before
/// specificity and source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    UserAgent,
    Author,
}

/// Whether the author origin is applied.
///
/// The product resolves the `purr.author-styles` capability and passes the
/// result here. When it is disabled the author sheet is not parsed and the
/// user-agent origin alone applies, a predictable degrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorStyles {
    Enabled,
    Disabled,
}

/// A property of the M2 set.
///
/// The set holds only longhands. The `margin` and `padding` shorthands are
/// expanded into their four longhands during parsing and are not stored as a
/// property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyId {
    Display,
    Width,
    Height,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,
    BackgroundColor,
    Color,
    FontSize,
    FontFamily,
    LineHeight,
}

impl PropertyId {
    /// Every property of the M2 set in a stable index order.
    ///
    /// The cascade and computed-style phase indexes a dense per-property array by
    /// `index`, and iterates the set through this array.
    pub const ALL: [PropertyId; 16] = [
        PropertyId::Display,
        PropertyId::Width,
        PropertyId::Height,
        PropertyId::MarginTop,
        PropertyId::MarginRight,
        PropertyId::MarginBottom,
        PropertyId::MarginLeft,
        PropertyId::PaddingTop,
        PropertyId::PaddingRight,
        PropertyId::PaddingBottom,
        PropertyId::PaddingLeft,
        PropertyId::BackgroundColor,
        PropertyId::Color,
        PropertyId::FontSize,
        PropertyId::FontFamily,
        PropertyId::LineHeight,
    ];

    /// The dense index of this property, matching its position in `ALL`.
    pub fn index(self) -> usize {
        match self {
            PropertyId::Display => 0,
            PropertyId::Width => 1,
            PropertyId::Height => 2,
            PropertyId::MarginTop => 3,
            PropertyId::MarginRight => 4,
            PropertyId::MarginBottom => 5,
            PropertyId::MarginLeft => 6,
            PropertyId::PaddingTop => 7,
            PropertyId::PaddingRight => 8,
            PropertyId::PaddingBottom => 9,
            PropertyId::PaddingLeft => 10,
            PropertyId::BackgroundColor => 11,
            PropertyId::Color => 12,
            PropertyId::FontSize => 13,
            PropertyId::FontFamily => 14,
            PropertyId::LineHeight => 15,
        }
    }
}

/// Specificity of one selector as a non-overflowing triple.
///
/// The fields are ordered id, class, then type, so the derived ordering compares
/// them in cascade priority. The counts saturate, so a pathological selector
/// cannot overflow the representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    id: u16,
    class: u16,
    type_component: u16,
}

impl Specificity {
    fn new(id: u16, class: u16, type_component: u16) -> Self {
        Self {
            id,
            class,
            type_component,
        }
    }

    pub fn id(self) -> u16 {
        self.id
    }

    pub fn class(self) -> u16 {
        self.class
    }

    pub fn type_component(self) -> u16 {
        self.type_component
    }
}

/// One compound selector.
///
/// A selector has an optional type, zero or more classes, and an optional id. It
/// carries no combinator or pseudo component in the M2 subset. Its specificity is
/// computed once at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    type_name: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
    specificity: Specificity,
}

impl Selector {
    pub fn type_name(&self) -> Option<&str> {
        self.type_name.as_deref()
    }

    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn specificity(&self) -> Specificity {
        self.specificity
    }
}

/// One declaration record.
///
/// The record holds a property, its serialized specified value, an importance
/// flag, its source-order index, and a validity flag. An invalid record is kept
/// in source order so the cascade can skip it deterministically; a syntactically
/// unusable declaration is dropped during parsing and never becomes a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    property: PropertyId,
    value: String,
    important: bool,
    source_order: u32,
    valid: bool,
}

impl Declaration {
    pub fn property(&self) -> PropertyId {
        self.property
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn important(&self) -> bool {
        self.important
    }

    pub fn source_order(&self) -> u32 {
        self.source_order
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

/// One style rule: a selector list and its declaration block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRule {
    selectors: Vec<Selector>,
    declarations: Vec<Declaration>,
}

impl StyleRule {
    pub fn selectors(&self) -> &[Selector] {
        &self.selectors
    }

    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }
}

/// An immutable parsed stylesheet of one origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stylesheet {
    origin: Origin,
    rules: Vec<StyleRule>,
}

impl Stylesheet {
    fn empty(origin: Origin) -> Self {
        Self {
            origin,
            rules: Vec::new(),
        }
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    pub fn rules(&self) -> &[StyleRule] {
        &self.rules
    }
}

/// Parses stylesheet text into an immutable stylesheet of the given origin.
///
/// Recovers by dropping: an invalid rule or declaration is dropped and the valid
/// ones survive. A per-rule or per-sheet cap rejects an over-limit rule while the
/// previously committed rules are preserved. Never panics on malformed input.
pub fn parse_stylesheet(source: &str, origin: Origin) -> Stylesheet {
    let tokens = tokenize(source);
    let mut parser = Parser {
        tokens: &tokens,
        position: 0,
        source_order: 0,
    };

    let mut rules = Vec::new();
    loop {
        match parser.next_rule() {
            RuleOutcome::Rule(rule) => {
                if rules.len() < MAX_RULES_PER_SHEET {
                    rules.push(rule);
                }
            }
            RuleOutcome::Dropped => {}
            RuleOutcome::End => break,
        }
    }

    Stylesheet { origin, rules }
}

/// Parses one embedded author `<style>` into the author origin.
///
/// Parses only when author styles are enabled; otherwise returns an empty author
/// stylesheet so the user-agent origin alone applies.
pub fn parse_author_stylesheet(source: &str, availability: AuthorStyles) -> Stylesheet {
    match availability {
        AuthorStyles::Enabled => parse_stylesheet(source, Origin::Author),
        AuthorStyles::Disabled => Stylesheet::empty(Origin::Author),
    }
}

enum RuleOutcome {
    Rule(StyleRule),
    Dropped,
    End,
}

const EOF: CssToken = CssToken::Eof;

struct Parser<'a> {
    tokens: &'a [CssToken],
    position: usize,
    source_order: u32,
}

impl Parser<'_> {
    fn next_rule(&mut self) -> RuleOutcome {
        if matches!(self.peek(), CssToken::Eof) {
            return RuleOutcome::End;
        }

        let Some(selectors) = self.parse_selector_list() else {
            self.skip_block();
            return RuleOutcome::Dropped;
        };

        let Some(declarations) = self.parse_declaration_block() else {
            return RuleOutcome::Dropped;
        };

        RuleOutcome::Rule(StyleRule {
            selectors,
            declarations,
        })
    }

    /// Parses the selector list up to the opening brace.
    ///
    /// Returns `None` when a selector is malformed, the list exceeds its cap, or
    /// no block opens, so the caller drops the rule.
    fn parse_selector_list(&mut self) -> Option<Vec<Selector>> {
        let mut selectors = Vec::new();
        loop {
            let selector = self.parse_selector()?;
            if selectors.len() >= MAX_SELECTORS_PER_LIST {
                return None;
            }
            selectors.push(selector);

            match self.peek() {
                CssToken::Comma => self.advance(),
                CssToken::LeftBrace => {
                    self.advance();
                    return Some(selectors);
                }
                _ => return None,
            }
        }
    }

    fn parse_selector(&mut self) -> Option<Selector> {
        let mut type_name = None;
        let mut classes = Vec::new();
        let mut id = None;
        let mut saw_component = false;

        if let CssToken::Ident(name) = self.peek() {
            type_name = Some(name.clone());
            saw_component = true;
            self.advance();
        }

        loop {
            match self.peek() {
                CssToken::Delim('.') => {
                    self.advance();
                    let CssToken::Ident(name) = self.peek() else {
                        return None;
                    };
                    classes.push(name.clone());
                    saw_component = true;
                    self.advance();
                }
                CssToken::Hash(name) => {
                    id = Some(name.clone());
                    saw_component = true;
                    self.advance();
                }
                _ => break,
            }
        }

        if !saw_component {
            return None;
        }

        let selector = Selector {
            specificity: specificity_of(id.is_some(), classes.len(), type_name.is_some()),
            type_name,
            classes,
            id,
        };
        if serialized_selector_len(&selector) > MAX_SELECTOR_LEN {
            return None;
        }
        Some(selector)
    }

    /// Parses the declaration block up to the closing brace.
    ///
    /// Returns `None` when the rule exceeds the declaration cap, so the caller
    /// drops the whole rule. A single invalid declaration is dropped and parsing
    /// continues with the next one.
    fn parse_declaration_block(&mut self) -> Option<Vec<Declaration>> {
        let mut declarations = Vec::new();
        let mut source_count = 0usize;
        loop {
            match self.peek() {
                CssToken::RightBrace => {
                    self.advance();
                    return Some(declarations);
                }
                CssToken::Eof => return Some(declarations),
                CssToken::Semicolon => self.advance(),
                _ => {
                    source_count = source_count.saturating_add(1);
                    if source_count > MAX_DECLARATIONS_PER_RULE {
                        self.skip_to_block_end();
                        return None;
                    }
                    self.parse_declaration(&mut declarations);
                }
            }
        }
    }

    /// Parses one declaration and appends its longhands, or drops it.
    fn parse_declaration(&mut self, out: &mut Vec<Declaration>) {
        let CssToken::Ident(name) = self.peek() else {
            self.skip_declaration();
            return;
        };
        let name = name.clone();
        self.advance();

        if !matches!(self.peek(), CssToken::Colon) {
            self.skip_declaration();
            return;
        }
        self.advance();

        let value_tokens = self.take_value_tokens();
        let Some(target) = resolve_property(&name) else {
            return;
        };

        let (component_tokens, important) = split_importance(&value_tokens);
        match target {
            PropertyTarget::Longhand(property) => {
                self.push_longhand(out, property, component_tokens, important);
            }
            PropertyTarget::MarginShorthand => {
                self.push_box_shorthand(out, MARGIN_SIDES, component_tokens, important);
            }
            PropertyTarget::PaddingShorthand => {
                self.push_box_shorthand(out, PADDING_SIDES, component_tokens, important);
            }
        }
    }

    fn push_longhand(
        &mut self,
        out: &mut Vec<Declaration>,
        property: PropertyId,
        component_tokens: &[CssToken],
        important: bool,
    ) {
        let value = serialize_value(component_tokens);
        if value.is_empty() || value.len() > MAX_VALUE_LEN {
            return;
        }
        out.push(self.declaration(property, value, important));
    }

    fn push_box_shorthand(
        &mut self,
        out: &mut Vec<Declaration>,
        sides: [PropertyId; 4],
        component_tokens: &[CssToken],
        important: bool,
    ) {
        let Some(values) = expand_box_shorthand(component_tokens) else {
            return;
        };
        for (property, value) in sides.into_iter().zip(values) {
            if value.len() > MAX_VALUE_LEN {
                continue;
            }
            out.push(self.declaration(property, value, important));
        }
    }

    fn declaration(&mut self, property: PropertyId, value: String, important: bool) -> Declaration {
        let source_order = self.source_order;
        self.source_order = self.source_order.saturating_add(1);
        let valid = !value.is_empty();
        Declaration {
            property,
            value,
            important,
            source_order,
            valid,
        }
    }

    /// Collects the value tokens up to the terminating semicolon or brace.
    fn take_value_tokens(&mut self) -> Vec<CssToken> {
        let mut value = Vec::new();
        loop {
            match self.peek() {
                CssToken::Semicolon => {
                    self.advance();
                    return value;
                }
                CssToken::RightBrace | CssToken::Eof => return value,
                other => {
                    value.push(other.clone());
                    self.advance();
                }
            }
        }
    }

    /// Skips a malformed declaration up to the next semicolon or brace.
    fn skip_declaration(&mut self) {
        loop {
            match self.peek() {
                CssToken::Semicolon => {
                    self.advance();
                    return;
                }
                CssToken::RightBrace | CssToken::Eof => return,
                _ => self.advance(),
            }
        }
    }

    /// Skips an entire malformed rule up to and past its closing brace.
    fn skip_block(&mut self) {
        while !matches!(self.peek(), CssToken::LeftBrace | CssToken::Eof) {
            self.advance();
        }
        self.skip_to_block_end();
    }

    /// Skips from inside a block to just past its closing brace.
    fn skip_to_block_end(&mut self) {
        if matches!(self.peek(), CssToken::LeftBrace) {
            self.advance();
        }
        loop {
            match self.peek() {
                CssToken::RightBrace => {
                    self.advance();
                    return;
                }
                CssToken::Eof => return,
                _ => self.advance(),
            }
        }
    }

    fn peek(&self) -> &CssToken {
        self.tokens.get(self.position).unwrap_or(&EOF)
    }

    fn advance(&mut self) {
        self.position = self.position.saturating_add(1);
    }
}

enum PropertyTarget {
    Longhand(PropertyId),
    MarginShorthand,
    PaddingShorthand,
}

const MARGIN_SIDES: [PropertyId; 4] = [
    PropertyId::MarginTop,
    PropertyId::MarginRight,
    PropertyId::MarginBottom,
    PropertyId::MarginLeft,
];

const PADDING_SIDES: [PropertyId; 4] = [
    PropertyId::PaddingTop,
    PropertyId::PaddingRight,
    PropertyId::PaddingBottom,
    PropertyId::PaddingLeft,
];

fn resolve_property(name: &str) -> Option<PropertyTarget> {
    let target = match name {
        "display" => PropertyTarget::Longhand(PropertyId::Display),
        "width" => PropertyTarget::Longhand(PropertyId::Width),
        "height" => PropertyTarget::Longhand(PropertyId::Height),
        "margin" => PropertyTarget::MarginShorthand,
        "margin-top" => PropertyTarget::Longhand(PropertyId::MarginTop),
        "margin-right" => PropertyTarget::Longhand(PropertyId::MarginRight),
        "margin-bottom" => PropertyTarget::Longhand(PropertyId::MarginBottom),
        "margin-left" => PropertyTarget::Longhand(PropertyId::MarginLeft),
        "padding" => PropertyTarget::PaddingShorthand,
        "padding-top" => PropertyTarget::Longhand(PropertyId::PaddingTop),
        "padding-right" => PropertyTarget::Longhand(PropertyId::PaddingRight),
        "padding-bottom" => PropertyTarget::Longhand(PropertyId::PaddingBottom),
        "padding-left" => PropertyTarget::Longhand(PropertyId::PaddingLeft),
        "background-color" => PropertyTarget::Longhand(PropertyId::BackgroundColor),
        "color" => PropertyTarget::Longhand(PropertyId::Color),
        "font-size" => PropertyTarget::Longhand(PropertyId::FontSize),
        "font-family" => PropertyTarget::Longhand(PropertyId::FontFamily),
        "line-height" => PropertyTarget::Longhand(PropertyId::LineHeight),
        _ => return None,
    };
    Some(target)
}

/// Splits a trailing `! important` from the value component tokens.
fn split_importance(value_tokens: &[CssToken]) -> (&[CssToken], bool) {
    let mut length = value_tokens.len();
    if length >= 2 {
        let last = &value_tokens[length - 1];
        let bang = &value_tokens[length - 2];
        let is_important =
            matches!(last, CssToken::Ident(name) if name.eq_ignore_ascii_case("important"));
        if is_important && matches!(bang, CssToken::Delim('!')) {
            length -= 2;
            return (&value_tokens[..length], true);
        }
    }
    (value_tokens, false)
}

/// Expands a box shorthand into top, right, bottom, and left values.
///
/// Accepts one to four component tokens with the standard side rules. Any other
/// count is invalid, so the shorthand is dropped.
fn expand_box_shorthand(component_tokens: &[CssToken]) -> Option<[String; 4]> {
    let sides: Vec<String> = component_tokens.iter().map(serialize_token).collect();
    match sides.as_slice() {
        [all] => Some([all.clone(), all.clone(), all.clone(), all.clone()]),
        [block, inline] => Some([block.clone(), inline.clone(), block.clone(), inline.clone()]),
        [top, inline, bottom] => {
            Some([top.clone(), inline.clone(), bottom.clone(), inline.clone()])
        }
        [top, right, bottom, left] => {
            Some([top.clone(), right.clone(), bottom.clone(), left.clone()])
        }
        _ => None,
    }
}

fn serialize_value(component_tokens: &[CssToken]) -> String {
    let parts: Vec<String> = component_tokens.iter().map(serialize_token).collect();
    parts.join(" ")
}

fn serialize_token(token: &CssToken) -> String {
    match token {
        CssToken::Ident(name) => name.clone(),
        CssToken::Hash(name) => format!("#{name}"),
        CssToken::Number(value) => value.clone(),
        CssToken::Dimension { value, unit } => format!("{value}{unit}"),
        CssToken::Percentage(value) => format!("{value}%"),
        CssToken::StringToken(value) => value.clone(),
        CssToken::Delim(character) => character.to_string(),
        CssToken::Comma => ",".to_owned(),
        CssToken::Colon => ":".to_owned(),
        CssToken::Semicolon => ";".to_owned(),
        CssToken::LeftBrace => "{".to_owned(),
        CssToken::RightBrace => "}".to_owned(),
        CssToken::Eof => String::new(),
    }
}

fn specificity_of(has_id: bool, class_count: usize, has_type: bool) -> Specificity {
    Specificity::new(
        u16::from(has_id),
        u16::try_from(class_count).unwrap_or(u16::MAX),
        u16::from(has_type),
    )
}

fn serialized_selector_len(selector: &Selector) -> usize {
    let mut length = selector.type_name.as_ref().map_or(0, String::len);
    for class in &selector.classes {
        length = length.saturating_add(class.len().saturating_add(1));
    }
    if let Some(id) = &selector.id {
        length = length.saturating_add(id.len().saturating_add(1));
    }
    length
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_rule(sheet: &Stylesheet, class: &str) -> Option<StyleRule> {
        sheet
            .rules()
            .iter()
            .find(|rule| {
                rule.selectors()
                    .iter()
                    .any(|selector| selector.classes().iter().any(|name| name == class))
            })
            .cloned()
    }

    fn value_of(rule: &StyleRule, property: PropertyId) -> Option<&str> {
        rule.declarations()
            .iter()
            .find(|declaration| declaration.property() == property)
            .map(Declaration::value)
    }

    #[test]
    fn parses_a_class_rule_with_expanded_padding() {
        let sheet = parse_stylesheet(
            ".card { background-color: #eee; padding: 8px; }",
            Origin::Author,
        );

        let rule = class_rule(&sheet, "card").expect("the card rule exists");
        assert_eq!(rule.selectors().len(), 1);
        assert_eq!(rule.selectors()[0].classes(), &["card".to_owned()]);

        assert_eq!(value_of(&rule, PropertyId::BackgroundColor), Some("#eee"));
        assert_eq!(value_of(&rule, PropertyId::PaddingTop), Some("8px"));
        assert_eq!(value_of(&rule, PropertyId::PaddingRight), Some("8px"));
        assert_eq!(value_of(&rule, PropertyId::PaddingBottom), Some("8px"));
        assert_eq!(value_of(&rule, PropertyId::PaddingLeft), Some("8px"));
    }

    #[test]
    fn padding_expands_the_two_value_form() {
        let sheet = parse_stylesheet(".box { padding: 8px 4px; }", Origin::Author);
        let rule = class_rule(&sheet, "box").expect("the box rule exists");

        assert_eq!(value_of(&rule, PropertyId::PaddingTop), Some("8px"));
        assert_eq!(value_of(&rule, PropertyId::PaddingRight), Some("4px"));
        assert_eq!(value_of(&rule, PropertyId::PaddingBottom), Some("8px"));
        assert_eq!(value_of(&rule, PropertyId::PaddingLeft), Some("4px"));
    }

    #[test]
    fn specificity_orders_id_over_class_over_type() {
        let sheet = parse_stylesheet("#main { color: #111; }", Origin::Author);
        let id_specificity = sheet.rules()[0].selectors()[0].specificity();

        let sheet = parse_stylesheet(".card { color: #222; }", Origin::Author);
        let class_specificity = sheet.rules()[0].selectors()[0].specificity();

        let sheet = parse_stylesheet("p { color: #333; }", Origin::Author);
        let type_specificity = sheet.rules()[0].selectors()[0].specificity();

        assert!(id_specificity > class_specificity);
        assert!(class_specificity > type_specificity);
    }

    #[test]
    fn an_invalid_declaration_is_dropped_and_the_valid_ones_survive() {
        let sheet = parse_stylesheet(
            ".card { color: #111; bogus-property: 1; width: 20px; }",
            Origin::Author,
        );
        let rule = class_rule(&sheet, "card").expect("the card rule exists");

        assert_eq!(value_of(&rule, PropertyId::Color), Some("#111"));
        assert_eq!(value_of(&rule, PropertyId::Width), Some("20px"));
        assert_eq!(rule.declarations().len(), 2);
    }

    #[test]
    fn an_invalid_rule_is_dropped_and_the_following_rules_survive() {
        let sheet = parse_stylesheet("& { color: #111; } .kept { color: #222; }", Origin::Author);

        assert_eq!(sheet.rules().len(), 1);
        let kept = class_rule(&sheet, "kept").expect("the kept rule survives");
        assert_eq!(value_of(&kept, PropertyId::Color), Some("#222"));
    }

    #[test]
    fn a_selector_list_over_the_cap_drops_the_rule_and_keeps_prior() {
        let mut source = String::from("p { color: #111; }");
        for index in 0..(MAX_SELECTORS_PER_LIST + 1) {
            if index > 0 {
                source.push(',');
            }
            source.push_str(&format!(".c{index}"));
        }
        source.push_str(" { color: #222; }");
        source.push_str(".last { color: #333; }");

        let sheet = parse_stylesheet(&source, Origin::Author);

        assert_eq!(value_of(&sheet.rules()[0], PropertyId::Color), Some("#111"));
        let last = class_rule(&sheet, "last").expect("the last rule survives");
        assert_eq!(value_of(&last, PropertyId::Color), Some("#333"));
    }

    #[test]
    fn declarations_over_the_cap_drop_the_rule_and_keep_prior() {
        let mut source = String::from("p { color: #111; }");
        source.push_str(".big {");
        for _ in 0..(MAX_DECLARATIONS_PER_RULE + 1) {
            source.push_str(" width: 1px;");
        }
        source.push('}');
        source.push_str(".last { color: #333; }");

        let sheet = parse_stylesheet(&source, Origin::Author);

        assert_eq!(value_of(&sheet.rules()[0], PropertyId::Color), Some("#111"));
        assert!(class_rule(&sheet, "big").is_none());
        let last = class_rule(&sheet, "last").expect("the last rule survives");
        assert_eq!(value_of(&last, PropertyId::Color), Some("#333"));
    }

    #[test]
    fn a_selector_over_the_length_cap_drops_the_rule() {
        let long = "c".repeat(MAX_SELECTOR_LEN + 1);
        let source = format!(".{long} {{ color: #111; }} .kept {{ color: #222; }}");

        let sheet = parse_stylesheet(&source, Origin::Author);

        let kept = class_rule(&sheet, "kept").expect("the kept rule survives");
        assert_eq!(value_of(&kept, PropertyId::Color), Some("#222"));
        assert_eq!(sheet.rules().len(), 1);
    }

    #[test]
    fn an_important_declaration_is_flagged() {
        let sheet = parse_stylesheet(".card { color: #111 !important; }", Origin::Author);
        let rule = class_rule(&sheet, "card").expect("the card rule exists");
        let declaration = &rule.declarations()[0];

        assert_eq!(declaration.property(), PropertyId::Color);
        assert_eq!(declaration.value(), "#111");
        assert!(declaration.important());
    }

    #[test]
    fn disabled_author_styles_parse_to_an_empty_sheet() {
        let sheet = parse_author_stylesheet(".card { color: #111; }", AuthorStyles::Disabled);

        assert_eq!(sheet.origin(), Origin::Author);
        assert!(sheet.rules().is_empty());
    }
}
