// @file engines/purr/engine/src/html-tree-builder.rs
// @description Builds the engine DOM from the tokenizer output through the narrow mutation interface.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Minimal HTML tree builder.
//!
//! The tree builder pulls tokens from the tokenizer one at a time and mutates the
//! DOM through its narrow Phase 02 interface. It runs an explicit insertion-mode
//! state machine with no recursion: a token is dispatched to the current mode,
//! which either consumes it or advances the mode and reprocesses it a bounded
//! number of times. The two parsers are directly coupled, so a token is consumed
//! as soon as it is produced and no intermediate token queue exists.
//!
//! The builder is bounded and fail-closed. The stack of open elements has an
//! explicit depth cap checked with checked arithmetic before each element push,
//! and an over-limit nesting aborts the parse with a typed error instead of a
//! panic. Malformed but recoverable input (a stray end tag, an unexpected token
//! in a mode) is handled by the recovery path and never drops a well-formed
//! sibling.
//!
//! The M2 subset is standards mode only. It omits the adoption-agency algorithm,
//! the active-formatting list, foster parenting, tables, templates, select, and
//! foreign content. Embedded `<style>` content is captured as element text now;
//! a later phase parses it.

// Some items (the open-element cap and a few DOM read paths) are exercised only
// by the tests and by later phases, so they are otherwise unused in a non-test
// build. The parse entry point itself is a live consumer from the store.
#![allow(dead_code)]

use crate::dom_node::{Dom, DomError, NodeId};
use crate::html_tokenizer::{Attribute, EndTag, HtmlToken, StartTag, Tokenizer, TokenizerError};

/// Upper bound for the number of elements on the stack of open elements.
///
/// The stack depth tracks the current insertion nesting, so the cap bounds the
/// tree the builder can grow. It sits at the DOM depth cap, and the builder
/// checks it before each push so an over-limit nesting fails with `TooDeep`
/// before the DOM append reports the same condition.
pub const MAX_OPEN_ELEMENTS: usize = 512;

/// Upper bound for the number of mode hops one token may take.
///
/// A token that is reprocessed moves toward the body modes on each hop, so a
/// well-formed transition settles in a few hops. The bound is a defensive
/// backstop: a token that cannot settle is dropped so tree construction always
/// makes progress and a malformed transition cannot loop forever.
const MAX_MODE_HOPS: usize = 8;

/// Failure the tree builder reports when the parse aborts.
///
/// The builder owns these variants. They are engine-internal: the store recovers
/// from them and never forwards them across the seam, so the frozen seam contract
/// is unchanged. A tokenizer abort and a DOM mutation failure are carried as
/// their source errors; the open-element cap has its own variant.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("tokenization aborted: {0}")]
    Tokenize(#[from] TokenizerError),
    #[error("the open-element stack reached its maximum depth")]
    TooDeep,
    #[error("the document tree rejected a mutation: {0}")]
    Tree(#[from] DomError),
}

/// Parses source bytes into a DOM.
///
/// Runs the tokenizer and the tree builder in lockstep and returns the built
/// tree. Aborts with a `ParseError` when a tokenizer cap or the open-element cap
/// is breached; a recoverable malformed input does not abort. Never panics on
/// malformed or truncated input.
pub fn parse(source: &[u8]) -> Result<Dom, ParseError> {
    let mut tokenizer = Tokenizer::new(source);
    let mut builder = TreeBuilder::new();
    loop {
        let token = tokenizer.next_token()?;
        if matches!(token, HtmlToken::EndOfFile) {
            builder.finish();
            break;
        }
        builder.process(token)?;
    }
    Ok(builder.into_dom())
}

/// Insertion mode of the tree builder.
///
/// The M2 subset keeps only the modes the fixture-shaped input reaches. `Text`
/// holds character data for `<title>` and `<style>` and returns to the mode that
/// entered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertionMode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    Text,
    AfterBody,
}

/// Disposition of one token in the current mode.
enum Control {
    Consumed,
    Reprocess,
}

/// Builds one DOM from a token stream.
///
/// The builder owns the DOM, the current and original insertion mode, the stack
/// of open elements as `NodeId` handles, and the head element pointer. It mutates
/// the tree only through the DOM interface.
struct TreeBuilder {
    dom: Dom,
    mode: InsertionMode,
    original_mode: Option<InsertionMode>,
    open_elements: Vec<NodeId>,
    head: Option<NodeId>,
}

impl TreeBuilder {
    fn new() -> Self {
        Self {
            dom: Dom::new(),
            mode: InsertionMode::Initial,
            original_mode: None,
            open_elements: Vec::new(),
            head: None,
        }
    }

    fn into_dom(self) -> Dom {
        self.dom
    }

    fn process(&mut self, token: HtmlToken) -> Result<(), ParseError> {
        for _ in 0..MAX_MODE_HOPS {
            let control = match self.mode {
                InsertionMode::Initial => self.mode_initial(&token)?,
                InsertionMode::BeforeHtml => self.mode_before_html(&token)?,
                InsertionMode::BeforeHead => self.mode_before_head(&token)?,
                InsertionMode::InHead => self.mode_in_head(&token)?,
                InsertionMode::AfterHead => self.mode_after_head(&token)?,
                InsertionMode::InBody => self.mode_in_body(&token)?,
                InsertionMode::Text => self.mode_text(&token)?,
                InsertionMode::AfterBody => self.mode_after_body(&token)?,
            };
            if let Control::Consumed = control {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Closes an open `<title>` or `<style>` when the input ends inside it.
    fn finish(&mut self) {
        if self.mode == InsertionMode::Text {
            self.open_elements.pop();
            self.mode = self.original_mode.take().unwrap_or(InsertionMode::InBody);
        }
    }

    fn mode_initial(&mut self, token: &HtmlToken) -> Result<Control, ParseError> {
        match token {
            HtmlToken::Doctype(doctype) => {
                let name = doctype.name.clone().unwrap_or_default();
                let node = self.dom.create_doctype(&name)?;
                let root = self.dom.root();
                self.dom.append_child(root, node)?;
                self.mode = InsertionMode::BeforeHtml;
                Ok(Control::Consumed)
            }
            HtmlToken::Comment(data) => {
                self.insert_comment(data)?;
                Ok(Control::Consumed)
            }
            HtmlToken::Character(cp) if is_space(cp.get()) => Ok(Control::Consumed),
            _ => {
                self.mode = InsertionMode::BeforeHtml;
                Ok(Control::Reprocess)
            }
        }
    }

    fn mode_before_html(&mut self, token: &HtmlToken) -> Result<Control, ParseError> {
        match token {
            HtmlToken::Doctype(_) => Ok(Control::Consumed),
            HtmlToken::Comment(data) => {
                self.insert_comment(data)?;
                Ok(Control::Consumed)
            }
            HtmlToken::Character(cp) if is_space(cp.get()) => Ok(Control::Consumed),
            HtmlToken::StartTag(tag) if tag.name == "html" => {
                self.insert_element(&tag.name, &tag.attributes)?;
                self.mode = InsertionMode::BeforeHead;
                Ok(Control::Consumed)
            }
            _ => {
                self.insert_element("html", &[])?;
                self.mode = InsertionMode::BeforeHead;
                Ok(Control::Reprocess)
            }
        }
    }

    fn mode_before_head(&mut self, token: &HtmlToken) -> Result<Control, ParseError> {
        match token {
            HtmlToken::Doctype(_) => Ok(Control::Consumed),
            HtmlToken::Comment(data) => {
                self.insert_comment(data)?;
                Ok(Control::Consumed)
            }
            HtmlToken::Character(cp) if is_space(cp.get()) => Ok(Control::Consumed),
            HtmlToken::StartTag(tag) if tag.name == "head" => {
                let head = self.insert_element(&tag.name, &tag.attributes)?;
                self.head = Some(head);
                self.mode = InsertionMode::InHead;
                Ok(Control::Consumed)
            }
            HtmlToken::StartTag(tag) if tag.name == "html" => Ok(Control::Consumed),
            _ => {
                let head = self.insert_element("head", &[])?;
                self.head = Some(head);
                self.mode = InsertionMode::InHead;
                Ok(Control::Reprocess)
            }
        }
    }

    fn mode_in_head(&mut self, token: &HtmlToken) -> Result<Control, ParseError> {
        match token {
            HtmlToken::Comment(data) => {
                self.insert_comment(data)?;
                Ok(Control::Consumed)
            }
            HtmlToken::Doctype(_) => Ok(Control::Consumed),
            HtmlToken::Character(cp) if is_space(cp.get()) => Ok(Control::Consumed),
            HtmlToken::StartTag(tag) => self.in_head_start(tag),
            HtmlToken::EndTag(tag) => self.in_head_end(tag),
            _ => {
                self.pop_head();
                self.mode = InsertionMode::AfterHead;
                Ok(Control::Reprocess)
            }
        }
    }

    fn in_head_start(&mut self, tag: &StartTag) -> Result<Control, ParseError> {
        match tag.name.as_str() {
            "title" | "style" => {
                self.insert_element(&tag.name, &tag.attributes)?;
                self.original_mode = Some(InsertionMode::InHead);
                self.mode = InsertionMode::Text;
                Ok(Control::Consumed)
            }
            "base" | "link" | "meta" => {
                self.insert_void(&tag.name, &tag.attributes)?;
                Ok(Control::Consumed)
            }
            "head" => Ok(Control::Consumed),
            _ => {
                self.pop_head();
                self.mode = InsertionMode::AfterHead;
                Ok(Control::Reprocess)
            }
        }
    }

    fn in_head_end(&mut self, tag: &EndTag) -> Result<Control, ParseError> {
        match tag.name.as_str() {
            "head" => {
                self.pop_head();
                self.mode = InsertionMode::AfterHead;
                Ok(Control::Consumed)
            }
            "body" | "html" | "br" => {
                self.pop_head();
                self.mode = InsertionMode::AfterHead;
                Ok(Control::Reprocess)
            }
            _ => Ok(Control::Consumed),
        }
    }

    fn mode_after_head(&mut self, token: &HtmlToken) -> Result<Control, ParseError> {
        match token {
            HtmlToken::Comment(data) => {
                self.insert_comment(data)?;
                Ok(Control::Consumed)
            }
            HtmlToken::Doctype(_) => Ok(Control::Consumed),
            HtmlToken::Character(cp) if is_space(cp.get()) => Ok(Control::Consumed),
            HtmlToken::StartTag(tag) if tag.name == "body" => {
                self.insert_element(&tag.name, &tag.attributes)?;
                self.mode = InsertionMode::InBody;
                Ok(Control::Consumed)
            }
            HtmlToken::StartTag(tag) if tag.name == "html" => Ok(Control::Consumed),
            _ => {
                self.insert_element("body", &[])?;
                self.mode = InsertionMode::InBody;
                Ok(Control::Reprocess)
            }
        }
    }

    fn mode_in_body(&mut self, token: &HtmlToken) -> Result<Control, ParseError> {
        match token {
            HtmlToken::Character(cp) => {
                self.insert_text(cp.get())?;
                Ok(Control::Consumed)
            }
            HtmlToken::Comment(data) => {
                self.insert_comment(data)?;
                Ok(Control::Consumed)
            }
            HtmlToken::Doctype(_) => Ok(Control::Consumed),
            HtmlToken::StartTag(tag) => self.in_body_start(tag),
            HtmlToken::EndTag(tag) => self.in_body_end(tag),
            HtmlToken::EndOfFile => Ok(Control::Consumed),
        }
    }

    fn in_body_start(&mut self, tag: &StartTag) -> Result<Control, ParseError> {
        let name = tag.name.as_str();
        match name {
            "html" | "head" | "body" => Ok(Control::Consumed),
            "title" | "style" => {
                self.insert_element(name, &tag.attributes)?;
                self.original_mode = Some(InsertionMode::InBody);
                self.mode = InsertionMode::Text;
                Ok(Control::Consumed)
            }
            _ if is_void(name) => {
                self.insert_void(name, &tag.attributes)?;
                Ok(Control::Consumed)
            }
            _ => {
                if closes_p(name) && self.has_p_in_scope() {
                    self.close_p();
                }
                self.insert_element(name, &tag.attributes)?;
                Ok(Control::Consumed)
            }
        }
    }

    fn in_body_end(&mut self, tag: &EndTag) -> Result<Control, ParseError> {
        match tag.name.as_str() {
            "body" => {
                self.mode = InsertionMode::AfterBody;
                Ok(Control::Consumed)
            }
            "html" => {
                self.mode = InsertionMode::AfterBody;
                Ok(Control::Reprocess)
            }
            "p" => {
                if self.has_p_in_scope() {
                    self.close_p();
                }
                Ok(Control::Consumed)
            }
            other => {
                self.pop_named(other);
                Ok(Control::Consumed)
            }
        }
    }

    fn mode_text(&mut self, token: &HtmlToken) -> Result<Control, ParseError> {
        match token {
            HtmlToken::Character(cp) => {
                self.insert_text(cp.get())?;
                Ok(Control::Consumed)
            }
            HtmlToken::EndTag(_) => {
                self.open_elements.pop();
                self.mode = self.original_mode.take().unwrap_or(InsertionMode::InBody);
                Ok(Control::Consumed)
            }
            _ => {
                self.open_elements.pop();
                self.mode = self.original_mode.take().unwrap_or(InsertionMode::InBody);
                Ok(Control::Reprocess)
            }
        }
    }

    fn mode_after_body(&mut self, token: &HtmlToken) -> Result<Control, ParseError> {
        match token {
            HtmlToken::Comment(data) => {
                self.insert_comment(data)?;
                Ok(Control::Consumed)
            }
            HtmlToken::Character(cp) if is_space(cp.get()) => Ok(Control::Consumed),
            HtmlToken::EndTag(tag) if tag.name == "html" => Ok(Control::Consumed),
            _ => {
                self.mode = InsertionMode::InBody;
                Ok(Control::Reprocess)
            }
        }
    }

    /// Inserts an element as the last child of the current node and pushes it.
    ///
    /// Fails closed with `TooDeep` when the open-element stack is at its cap,
    /// before it creates the node.
    fn insert_element(
        &mut self,
        name: &str,
        attributes: &[Attribute],
    ) -> Result<NodeId, ParseError> {
        if self.open_elements.len() >= MAX_OPEN_ELEMENTS {
            return Err(ParseError::TooDeep);
        }

        let parent = self.current_parent();
        let element = self.dom.create_element(name)?;
        for attribute in attributes {
            self.dom
                .set_attribute(element, &attribute.name, &attribute.value)?;
        }
        self.dom.append_child(parent, element)?;
        self.open_elements.push(element);
        Ok(element)
    }

    /// Inserts a void element without pushing it onto the open-element stack.
    fn insert_void(&mut self, name: &str, attributes: &[Attribute]) -> Result<(), ParseError> {
        let parent = self.current_parent();
        let element = self.dom.create_element(name)?;
        for attribute in attributes {
            self.dom
                .set_attribute(element, &attribute.name, &attribute.value)?;
        }
        self.dom.append_child(parent, element)?;
        Ok(())
    }

    /// Inserts a comment as the last child of the current node.
    fn insert_comment(&mut self, data: &str) -> Result<(), ParseError> {
        let parent = self.current_parent();
        let comment = self.dom.create_comment(data)?;
        self.dom.append_child(parent, comment)?;
        Ok(())
    }

    /// Appends one character to the current node, coalescing adjacent text.
    ///
    /// The character is encoded on the stack, so a per-character insertion does
    /// not allocate a new string. The DOM coalesces the run into one text node.
    fn insert_text(&mut self, value: char) -> Result<(), ParseError> {
        let parent = self.current_parent();
        let mut buffer = [0u8; 4];
        let text = value.encode_utf8(&mut buffer);
        self.dom.append_text(parent, text)?;
        Ok(())
    }

    /// The node new children attach to: the top of the stack, or the root.
    fn current_parent(&self) -> NodeId {
        self.open_elements
            .last()
            .copied()
            .unwrap_or_else(|| self.dom.root())
    }

    /// Pops the head element and anything still above it.
    ///
    /// A `<title>` or `<style>` pushes and pops within `Text` mode, so the head
    /// is normally the top when it closes; the loop is defensive.
    fn pop_head(&mut self) {
        while let Some(top) = self.open_elements.last().copied() {
            self.open_elements.pop();
            if Some(top) == self.head {
                break;
            }
        }
    }

    /// Whether a `<p>` is open in the current scope.
    ///
    /// The M2 subset has no scope-boundary elements (no button, table, or
    /// template), so the check scans the open stack for a `<p>`.
    fn has_p_in_scope(&self) -> bool {
        self.open_elements
            .iter()
            .rev()
            .any(|&element| self.dom.local_name(element) == Some("p"))
    }

    /// Pops elements up to and including the nearest open `<p>`.
    fn close_p(&mut self) {
        while let Some(top) = self.open_elements.last().copied() {
            let is_p = self.dom.local_name(top) == Some("p");
            self.open_elements.pop();
            if is_p {
                break;
            }
        }
    }

    /// Pops elements up to and including the nearest open element with `name`.
    ///
    /// An end tag with no matching open element is ignored, so a stray end tag
    /// does not drop a well-formed sibling.
    fn pop_named(&mut self, name: &str) {
        let present = self
            .open_elements
            .iter()
            .rev()
            .any(|&element| self.dom.local_name(element) == Some(name));
        if !present {
            return;
        }

        while let Some(top) = self.open_elements.last().copied() {
            let matches = self.dom.local_name(top) == Some(name);
            self.open_elements.pop();
            if matches {
                break;
            }
        }
    }
}

fn is_space(value: char) -> bool {
    matches!(value, ' ' | '\t' | '\n' | '\u{000C}' | '\r')
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Whether a start tag implicitly closes an open `<p>`.
fn closes_p(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "details"
            | "div"
            | "dl"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "header"
            | "hgroup"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "section"
            | "summary"
            | "table"
            | "ul"
            | "li"
            | "pre"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom_node::NodeKind;

    fn element_child(dom: &Dom, parent: NodeId, name: &str) -> Option<NodeId> {
        dom.children(parent)?
            .iter()
            .copied()
            .find(|&child| dom.local_name(child) == Some(name))
    }

    fn only_text(dom: &Dom, parent: NodeId) -> Option<String> {
        let children = dom.children(parent)?;
        if children.len() != 1 {
            return None;
        }
        dom.text_data(children[0]).map(str::to_owned)
    }

    #[test]
    fn parses_the_fixture_shape_into_the_expected_tree() {
        let dom = parse(
            b"<!doctype html><html><head><title>t</title></head><body><p>hi</p></body></html>",
        )
        .expect("the fixture-shaped input parses");

        let html = element_child(&dom, dom.root(), "html").expect("html under the root");
        let head = element_child(&dom, html, "head").expect("head under html");
        let title = element_child(&dom, head, "title").expect("title under head");
        assert_eq!(only_text(&dom, title).as_deref(), Some("t"));

        let body = element_child(&dom, html, "body").expect("body under html");
        let paragraph = element_child(&dom, body, "p").expect("p under body");
        assert_eq!(only_text(&dom, paragraph).as_deref(), Some("hi"));
    }

    #[test]
    fn a_card_structure_carries_its_attributes_and_text() {
        let dom = parse(b"<div class=\"card\"><p>body</p></div>").expect("the snippet parses");

        let html = element_child(&dom, dom.root(), "html").expect("implicit html");
        let body = element_child(&dom, html, "body").expect("implicit body");
        let card = element_child(&dom, body, "div").expect("div under body");

        let attributes = dom.attributes(card).expect("div is an element");
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes[0].name, "class");
        assert_eq!(attributes[0].value, "card");

        let paragraph = element_child(&dom, card, "p").expect("p under the card");
        assert_eq!(only_text(&dom, paragraph).as_deref(), Some("body"));
    }

    #[test]
    fn a_second_p_closes_the_first_into_a_sibling() {
        let dom = parse(b"<p>a<p>b").expect("the snippet parses");

        let html = element_child(&dom, dom.root(), "html").expect("implicit html");
        let body = element_child(&dom, html, "body").expect("implicit body");

        let paragraphs: Vec<NodeId> = dom
            .children(body)
            .expect("body resolves")
            .iter()
            .copied()
            .filter(|&child| dom.local_name(child) == Some("p"))
            .collect();
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(only_text(&dom, paragraphs[0]).as_deref(), Some("a"));
        assert_eq!(only_text(&dom, paragraphs[1]).as_deref(), Some("b"));

        let first = paragraphs[0];
        assert_eq!(dom.parent(first), Some(body));
    }

    #[test]
    fn title_content_is_a_single_text_child() {
        let dom = parse(b"<title>a title</title>").expect("the snippet parses");

        let html = element_child(&dom, dom.root(), "html").expect("implicit html");
        let head = element_child(&dom, html, "head").expect("implicit head");
        let title = element_child(&dom, head, "title").expect("title under head");

        let children = dom.children(title).expect("title resolves");
        assert_eq!(children.len(), 1);
        assert_eq!(dom.kind(children[0]), Some(NodeKind::Text));
        assert_eq!(dom.text_data(children[0]), Some("a title"));
    }

    #[test]
    fn a_stray_end_tag_keeps_the_well_formed_siblings() {
        let dom = parse(b"<div>a</span>b</div>").expect("the malformed input recovers");

        let html = element_child(&dom, dom.root(), "html").expect("implicit html");
        let body = element_child(&dom, html, "body").expect("implicit body");
        let container = element_child(&dom, body, "div").expect("div under body");

        assert_eq!(only_text(&dom, container).as_deref(), Some("ab"));
    }

    #[test]
    fn deep_nesting_beyond_the_stack_cap_aborts() {
        let mut source = String::new();
        for _ in 0..(MAX_OPEN_ELEMENTS + 100) {
            source.push_str("<div>");
        }

        let result = parse(source.as_bytes());
        assert!(matches!(result, Err(ParseError::TooDeep)));
    }
}
