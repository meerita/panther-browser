// @file engines/purr/engine/src/dom-node.rs
// @description Defines the minimal engine DOM: node kinds, node identity, and the narrow mutation interface.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Minimal engine DOM.
//!
//! The DOM stores every node of one document in a generational-index arena in
//! the Document memory region. A node is named by a `NodeId`, a stable domain
//! identity plus generation, never a raw pointer, so a handle to a reused slot
//! stops resolving. The tree builder fills the DOM through a narrow interface
//! (create a node, append a child, set an attribute, append or coalesce text);
//! style, layout, and paint read it back.
//!
//! The tree is bounded: a node-count cap and a tree-depth cap are checked with
//! checked arithmetic before each insertion, and an over-limit mutation fails
//! closed with a typed error instead of a panic or a silent truncation.

// The tree builder (a later phase) is the first non-test consumer of the
// mutation and read interface. This phase adds the store and exercises the
// interface through the unit tests below, so the methods are otherwise unused
// in a non-test build.
#![allow(dead_code)]

use memory::{AccountingRegistry, Arena, ArenaId, Region};

/// Upper bound for the number of nodes in one document tree.
///
/// The DOM owns this limit and rejects a node creation that would exceed it. The
/// bound mirrors the graphics draw-command bound for the M2 slice, which renders
/// one small static local document; a later milestone raises it as the input
/// space grows.
pub const MAX_NODE_COUNT: usize = 65_536;

/// Upper bound for the depth of one document tree.
///
/// The DOM rejects an append that would place a node below this depth. The bound
/// keeps the ancestor walk and later recursive tree passes bounded on adversarial
/// nesting.
pub const MAX_TREE_DEPTH: usize = 512;

/// Kind of one DOM node.
///
/// The M2 DOM has no Web IDL wrappers, events, or foreign content, so the kind
/// set is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    DocumentType,
    Element,
    Text,
    Comment,
}

/// One element attribute as an ordered name/value pair.
///
/// Attributes keep insertion order, so a later phase reads them in the order the
/// tree builder set them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

/// Opaque identity of one node in a `Dom`.
///
/// The identity wraps the arena handle, so it carries the slot generation and
/// rejects a stale or out-of-range handle on read. Only a `Dom` constructs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(ArenaId);

/// Failure the DOM reports to its caller.
///
/// The DOM owns these variants. A later phase translates them into the store's
/// `DocumentError` at the point where a parse error surfaces to the seam. Each
/// message is a static, factual, non-secret string.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DomError {
    #[error("the DOM reached its maximum node count")]
    TooManyNodes,
    #[error("the DOM reached its maximum tree depth")]
    TooDeep,
    #[error("the node handle does not name a live node")]
    UnknownNode,
    #[error("the node is not an element")]
    NotAnElement,
}

/// Per-kind payload of one node.
///
/// A `Document` and an `Element` hold children through the common node fields; an
/// `Element` also holds its local name and ordered attributes. `Text`,
/// `Comment`, and `DocumentType` hold their character data or name.
enum NodeData {
    Document,
    DocumentType {
        name: String,
    },
    Element {
        local_name: String,
        attributes: Vec<Attribute>,
    },
    Text {
        data: String,
    },
    Comment {
        data: String,
    },
}

/// One node in the tree.
///
/// The node holds its payload, an optional parent link, and its children in
/// document order. Links are `NodeId` handles, never pointers.
struct Node {
    data: NodeData,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
}

/// Owns the node arena and the document root of one document tree.
///
/// Nodes live in a generational-index arena accounted to the Document region.
/// The DOM issues opaque `NodeId` handles and enforces the node-count and depth
/// caps on every mutation.
pub struct Dom {
    nodes: Arena<Node>,
    accounting: AccountingRegistry,
    root: NodeId,
}

impl Dom {
    /// Creates a DOM with a single `Document` root.
    pub fn new() -> Self {
        let accounting = AccountingRegistry::new();
        let mut nodes = Arena::new();
        let root = NodeId(nodes.insert_accounted(
            Node {
                data: NodeData::Document,
                parent: None,
                children: Vec::new(),
            },
            Region::Document,
            &accounting,
        ));

        Self {
            nodes,
            accounting,
            root,
        }
    }

    /// The document root.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// The number of live nodes, including the root.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Creates a detached element with the given local name.
    ///
    /// Fails closed with `TooManyNodes` when the tree is at the node-count cap.
    /// The element has no parent until it is appended.
    pub fn create_element(&mut self, local_name: &str) -> Result<NodeId, DomError> {
        self.insert_node(NodeData::Element {
            local_name: local_name.to_owned(),
            attributes: Vec::new(),
        })
    }

    /// Creates a detached comment node with the given data.
    pub fn create_comment(&mut self, data: &str) -> Result<NodeId, DomError> {
        self.insert_node(NodeData::Comment {
            data: data.to_owned(),
        })
    }

    /// Creates a detached doctype node with the given name.
    pub fn create_doctype(&mut self, name: &str) -> Result<NodeId, DomError> {
        self.insert_node(NodeData::DocumentType {
            name: name.to_owned(),
        })
    }

    /// Appends an existing node as the last child of a parent.
    ///
    /// Fails closed with `TooDeep` when the child would sit below the depth cap,
    /// and with `UnknownNode` when either handle is stale or out of range. The
    /// child keeps document order at the end of the parent's child list.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        let parent_depth = self.depth(parent).ok_or(DomError::UnknownNode)?;
        let child_depth = parent_depth.checked_add(1).ok_or(DomError::TooDeep)?;
        if child_depth > MAX_TREE_DEPTH {
            return Err(DomError::TooDeep);
        }

        self.nodes
            .get_mut(child.0)
            .ok_or(DomError::UnknownNode)?
            .parent = Some(parent);
        self.nodes
            .get_mut(parent.0)
            .ok_or(DomError::UnknownNode)?
            .children
            .push(child);
        Ok(())
    }

    /// Appends character data to a parent, coalescing adjacent text.
    ///
    /// When the parent's last child is a `Text` node, the data extends that node
    /// and no node is created. Otherwise a new `Text` node is created and
    /// appended, subject to the node-count and depth caps.
    pub fn append_text(&mut self, parent: NodeId, data: &str) -> Result<(), DomError> {
        if self.nodes.get(parent.0).is_none() {
            return Err(DomError::UnknownNode);
        }

        if self.coalesce_trailing_text(parent, data) {
            return Ok(());
        }

        let text = self.insert_node(NodeData::Text {
            data: data.to_owned(),
        })?;
        self.append_child(parent, text)
    }

    /// Sets an attribute on an element, preserving insertion order.
    ///
    /// An existing attribute of the same name keeps its position and takes the
    /// new value; a new attribute is appended. Fails with `NotAnElement` when the
    /// handle names a node that is not an element, and `UnknownNode` when the
    /// handle is stale or out of range.
    pub fn set_attribute(
        &mut self,
        element: NodeId,
        name: &str,
        value: &str,
    ) -> Result<(), DomError> {
        let node = self.nodes.get_mut(element.0).ok_or(DomError::UnknownNode)?;
        let NodeData::Element { attributes, .. } = &mut node.data else {
            return Err(DomError::NotAnElement);
        };

        if let Some(existing) = attributes
            .iter_mut()
            .find(|attribute| attribute.name == name)
        {
            existing.value = value.to_owned();
        } else {
            attributes.push(Attribute {
                name: name.to_owned(),
                value: value.to_owned(),
            });
        }
        Ok(())
    }

    /// The kind of a node, or `None` when the handle does not resolve.
    pub fn kind(&self, node: NodeId) -> Option<NodeKind> {
        let kind = match self.nodes.get(node.0)?.data {
            NodeData::Document => NodeKind::Document,
            NodeData::DocumentType { .. } => NodeKind::DocumentType,
            NodeData::Element { .. } => NodeKind::Element,
            NodeData::Text { .. } => NodeKind::Text,
            NodeData::Comment { .. } => NodeKind::Comment,
        };
        Some(kind)
    }

    /// The parent of a node, or `None` for the root or an unresolved handle.
    pub fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.nodes.get(node.0)?.parent
    }

    /// The children of a node in document order, or `None` when the handle does
    /// not resolve.
    pub fn children(&self, node: NodeId) -> Option<&[NodeId]> {
        Some(self.nodes.get(node.0)?.children.as_slice())
    }

    /// The local name of an element, or `None` for a non-element or an
    /// unresolved handle.
    pub fn local_name(&self, node: NodeId) -> Option<&str> {
        match &self.nodes.get(node.0)?.data {
            NodeData::Element { local_name, .. } => Some(local_name.as_str()),
            _ => None,
        }
    }

    /// The attributes of an element in insertion order, or `None` for a
    /// non-element or an unresolved handle.
    pub fn attributes(&self, node: NodeId) -> Option<&[Attribute]> {
        match &self.nodes.get(node.0)?.data {
            NodeData::Element { attributes, .. } => Some(attributes.as_slice()),
            _ => None,
        }
    }

    /// The character data of a text node, or `None` for another kind or an
    /// unresolved handle.
    pub fn text_data(&self, node: NodeId) -> Option<&str> {
        match &self.nodes.get(node.0)?.data {
            NodeData::Text { data } => Some(data.as_str()),
            _ => None,
        }
    }

    /// The data of a comment node, or `None` for another kind or an unresolved
    /// handle.
    pub fn comment_data(&self, node: NodeId) -> Option<&str> {
        match &self.nodes.get(node.0)?.data {
            NodeData::Comment { data } => Some(data.as_str()),
            _ => None,
        }
    }

    /// The name of a doctype node, or `None` for another kind or an unresolved
    /// handle.
    pub fn doctype_name(&self, node: NodeId) -> Option<&str> {
        match &self.nodes.get(node.0)?.data {
            NodeData::DocumentType { name } => Some(name.as_str()),
            _ => None,
        }
    }

    /// Inserts a node after the node-count cap check.
    fn insert_node(&mut self, data: NodeData) -> Result<NodeId, DomError> {
        if self.nodes.len() >= MAX_NODE_COUNT {
            return Err(DomError::TooManyNodes);
        }

        let id = self.nodes.insert_accounted(
            Node {
                data,
                parent: None,
                children: Vec::new(),
            },
            Region::Document,
            &self.accounting,
        );
        Ok(NodeId(id))
    }

    /// Extends the parent's trailing `Text` child with `data`.
    ///
    /// Returns whether the data was coalesced. A parent whose last child is not a
    /// `Text` node, or an unresolved handle, is not coalesced.
    fn coalesce_trailing_text(&mut self, parent: NodeId, data: &str) -> bool {
        let Some(last) = self.last_child(parent) else {
            return false;
        };
        let Some(node) = self.nodes.get_mut(last.0) else {
            return false;
        };
        let NodeData::Text { data: existing } = &mut node.data else {
            return false;
        };
        existing.push_str(data);
        true
    }

    /// The last child of a node, or `None` when it has none or does not resolve.
    fn last_child(&self, parent: NodeId) -> Option<NodeId> {
        self.nodes.get(parent.0)?.children.last().copied()
    }

    /// The depth of a node from the root, or `None` when the handle does not
    /// resolve.
    ///
    /// The root is depth zero. The walk is bounded by the depth cap, so a
    /// well-formed tree terminates and a hypothetical cycle cannot loop forever.
    fn depth(&self, node: NodeId) -> Option<usize> {
        let mut current = self.nodes.get(node.0)?;
        let mut depth = 0;
        while let Some(parent) = current.parent {
            depth += 1;
            if depth > MAX_TREE_DEPTH {
                break;
            }
            current = self.nodes.get(parent.0)?;
        }
        Some(depth)
    }
}

impl Default for Dom {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_element_and_text_read_back_in_order() {
        let mut dom = Dom::new();
        let root = dom.root();
        let element = dom.create_element("p").expect("under the node cap");
        dom.append_child(root, element)
            .expect("within the depth cap");
        dom.append_text(element, "hello").expect("appends text");

        let root_children = dom.children(root).expect("root resolves");
        assert_eq!(root_children, &[element]);
        assert_eq!(dom.kind(element), Some(NodeKind::Element));
        assert_eq!(dom.local_name(element), Some("p"));
        assert_eq!(dom.parent(element), Some(root));

        let element_children = dom.children(element).expect("element resolves");
        assert_eq!(element_children.len(), 1);
        let text = element_children[0];
        assert_eq!(dom.kind(text), Some(NodeKind::Text));
        assert_eq!(dom.text_data(text), Some("hello"));
        assert_eq!(dom.parent(text), Some(element));
    }

    #[test]
    fn adjacent_text_coalesces_into_one_node() {
        let mut dom = Dom::new();
        let element = dom.create_element("p").expect("under the node cap");
        dom.append_child(dom.root(), element).expect("within depth");
        let before = dom.node_count();

        dom.append_text(element, "foo").expect("appends text");
        dom.append_text(element, "bar").expect("coalesces text");

        let children = dom.children(element).expect("element resolves");
        assert_eq!(children.len(), 1);
        assert_eq!(dom.text_data(children[0]), Some("foobar"));
        assert_eq!(dom.node_count(), before + 1);
    }

    #[test]
    fn text_does_not_coalesce_across_an_element() {
        let mut dom = Dom::new();
        let element = dom.create_element("p").expect("under the node cap");
        dom.append_child(dom.root(), element).expect("within depth");

        dom.append_text(element, "before").expect("appends text");
        let inner = dom.create_element("span").expect("under the node cap");
        dom.append_child(element, inner).expect("within depth");
        dom.append_text(element, "after")
            .expect("appends a new text node");

        let children = dom.children(element).expect("element resolves");
        assert_eq!(children.len(), 3);
        assert_eq!(dom.kind(children[0]), Some(NodeKind::Text));
        assert_eq!(dom.kind(children[1]), Some(NodeKind::Element));
        assert_eq!(dom.kind(children[2]), Some(NodeKind::Text));
        assert_eq!(dom.text_data(children[2]), Some("after"));
    }

    #[test]
    fn node_kinds_round_trip_their_data() {
        let mut dom = Dom::new();
        let doctype = dom.create_doctype("html").expect("under the node cap");
        let comment = dom.create_comment("note").expect("under the node cap");
        dom.append_child(dom.root(), doctype).expect("within depth");
        dom.append_child(dom.root(), comment).expect("within depth");

        assert_eq!(dom.kind(dom.root()), Some(NodeKind::Document));
        assert_eq!(dom.kind(doctype), Some(NodeKind::DocumentType));
        assert_eq!(dom.doctype_name(doctype), Some("html"));
        assert_eq!(dom.kind(comment), Some(NodeKind::Comment));
        assert_eq!(dom.comment_data(comment), Some("note"));
    }

    #[test]
    fn out_of_range_handle_resolves_to_none() {
        let mut other = Dom::new();
        let foreign = other.create_element("div").expect("under the node cap");
        let dom = Dom::new();

        assert_eq!(dom.kind(foreign), None);
        assert_eq!(dom.children(foreign), None);
        assert_eq!(dom.parent(foreign), None);
    }

    #[test]
    fn attributes_preserve_insertion_order() {
        let mut dom = Dom::new();
        let element = dom.create_element("div").expect("under the node cap");
        dom.set_attribute(element, "id", "main").expect("sets id");
        dom.set_attribute(element, "class", "box")
            .expect("sets class");
        dom.set_attribute(element, "data-x", "1")
            .expect("sets data-x");

        let names: Vec<&str> = dom
            .attributes(element)
            .expect("element resolves")
            .iter()
            .map(|attribute| attribute.name.as_str())
            .collect();
        assert_eq!(names, ["id", "class", "data-x"]);

        dom.set_attribute(element, "id", "changed")
            .expect("updates id in place");
        let attributes = dom.attributes(element).expect("element resolves");
        assert_eq!(attributes.len(), 3);
        assert_eq!(attributes[0].name, "id");
        assert_eq!(attributes[0].value, "changed");
    }

    #[test]
    fn set_attribute_on_a_non_element_fails() {
        let mut dom = Dom::new();
        dom.append_text(dom.root(), "text").expect("appends text");
        let text = dom.children(dom.root()).expect("root resolves")[0];

        assert_eq!(
            dom.set_attribute(text, "id", "x"),
            Err(DomError::NotAnElement)
        );
    }

    #[test]
    fn node_count_cap_rejects_an_over_limit_creation() {
        let mut dom = Dom::new();
        for _ in 0..(MAX_NODE_COUNT - 1) {
            dom.create_element("div").expect("under the node cap");
        }
        assert_eq!(dom.node_count(), MAX_NODE_COUNT);

        assert_eq!(dom.create_element("div"), Err(DomError::TooManyNodes));
    }

    #[test]
    fn depth_cap_rejects_an_over_limit_append() {
        let mut dom = Dom::new();
        let mut parent = dom.root();
        for _ in 0..MAX_TREE_DEPTH {
            let child = dom.create_element("div").expect("under the node cap");
            dom.append_child(parent, child)
                .expect("within the depth cap");
            parent = child;
        }

        let too_deep = dom.create_element("div").expect("under the node cap");
        assert_eq!(dom.append_child(parent, too_deep), Err(DomError::TooDeep));
    }
}
