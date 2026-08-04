// @file products/panther/browser/src/tab.rs
// @description Defines the tab identity, content state, and tab value type.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Tab identity and per-tab content state.
//!
//! A tab is named by a stable [`TabId`] and holds one [`TabContent`]. The content
//! is either empty or an attached document, named by the opaque seam handle. The
//! handle stays internal to the product core; a consumer names a tab only by its
//! identity.

use purr_embedding::DocumentHandle;

use crate::address::Address;

/// Stable identity of a tab.
///
/// The identity comes from a monotonic counter owned by the tab model, so it is
/// never reused for the life of the model. It is a value the product holds across
/// mutations; a tab is always referenced by identity, never by collection index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u64);

impl TabId {
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Content state of a tab.
///
/// A tab starts `Empty` and moves to `Attached` when a document is attached to it.
/// The attached handle is opaque and stays inside the product core.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TabContent {
    Empty,
    Attached(DocumentHandle),
}

/// One tab: a stable identity, its current content state, and its committed address.
///
/// The committed address is `None` for a never-navigated tab, so a fresh tab shows
/// an empty field, and `Some` once anything has been submitted, including
/// `panther:blank`.
#[derive(Debug)]
pub struct Tab {
    id: TabId,
    content: TabContent,
    address: Option<Address>,
}

impl Tab {
    pub(crate) fn new(id: TabId) -> Self {
        Self {
            id,
            content: TabContent::Empty,
            address: None,
        }
    }

    pub fn id(&self) -> TabId {
        self.id
    }

    pub(crate) fn content(&self) -> &TabContent {
        &self.content
    }

    pub(crate) fn set_content(&mut self, content: TabContent) {
        self.content = content;
    }

    /// The committed address display string, or `None` for a never-navigated tab.
    pub fn address_text(&self) -> Option<&str> {
        self.address.as_ref().map(Address::as_str)
    }

    pub(crate) fn set_address(&mut self, address: Address) {
        self.address = Some(address);
    }
}
