// @file products/panther/browser/src/tab-model.rs
// @description Owns the tabs, the active-tab selector, and the document seam.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Tab model.
//!
//! The model owns the ordered tab collection, the active-tab selector, and one
//! shared [`DocumentSession`]. It drives the document-attachment seam per tab: a
//! tab holds an opaque handle, and the session owns the engine document. The seam
//! is stateless, so the model holds the lifecycle state (which tab is active and
//! what each tab has attached), not the seam.
//!
//! A tab is always referenced by [`TabId`], never by collection index, so a
//! removal never reassigns identity. Identities come from a monotonic counter and
//! are never reused. The model caches no produced frame; the consumer produces the
//! active frame on demand.

use purr_embedding::{
    DocumentFrame, DocumentGeneration, DocumentSession, ViewportGeometry, m2_demonstration_fixture,
};

use crate::address::{Address, AddressRoute};
use crate::core_error::CoreError;
use crate::tab::{Tab, TabContent, TabId};

/// Result of submitting an address.
///
/// An unparseable address and a parseable-but-unsupported address both yield the
/// identical `Rejected` outcome, so a caller cannot distinguish the two (uniform
/// failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressOutcome {
    Committed,
    Rejected,
}

/// Owns the tabs, the active tab, and the shared document seam.
pub struct TabModel {
    session: DocumentSession,
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: u64,
}

impl TabModel {
    pub fn new() -> Self {
        Self {
            session: DocumentSession::new(),
            tabs: Vec::new(),
            active: None,
            next_id: 0,
        }
    }

    /// Opens a new empty tab and makes it the active tab.
    ///
    /// The identity comes from the monotonic counter, so it is never reused.
    pub fn open_tab(&mut self) -> TabId {
        let id = TabId::new(self.next_id);
        self.next_id += 1;
        self.tabs.push(Tab::new(id));
        self.active = Some(id);
        id
    }

    /// Attaches a document to a tab from local source bytes.
    ///
    /// Fails with `UnknownTab` when the identity names no tab. Re-attaching a tab
    /// that already holds a document detaches the old document first, so the seam
    /// does not keep a document no tab references.
    pub fn attach(&mut self, tab: TabId, source: &[u8]) -> Result<(), CoreError> {
        let Some(position) = self.tabs.iter().position(|candidate| candidate.id() == tab) else {
            return Err(CoreError::UnknownTab);
        };

        if let TabContent::Attached(handle) = *self.tabs[position].content() {
            self.session.detach(handle);
        }

        let handle = self.session.attach(source)?;
        self.tabs[position].set_content(TabContent::Attached(handle));
        Ok(())
    }

    /// Submits address text for the active tab.
    ///
    /// The text is untrusted input, so it is parsed before any use. A missing
    /// active tab, an unparseable address, and an unsupported route all leave every
    /// committed address unchanged and report `Rejected`. `panther:blank` detaches
    /// the active tab's document and commits the address; `panther:demo` attaches
    /// the demonstration document and commits the address.
    pub fn submit_address(&mut self, text: &str) -> Result<AddressOutcome, CoreError> {
        let Some(active) = self.active else {
            return Ok(AddressOutcome::Rejected);
        };

        let Some(position) = self
            .tabs
            .iter()
            .position(|candidate| candidate.id() == active)
        else {
            return Ok(AddressOutcome::Rejected);
        };

        let Some(address) = Address::parse(text) else {
            return Ok(AddressOutcome::Rejected);
        };

        match address.route() {
            AddressRoute::Blank => {
                if let TabContent::Attached(handle) = *self.tabs[position].content() {
                    self.session.detach(handle);
                }
                self.tabs[position].set_content(TabContent::Empty);
                self.tabs[position].set_address(address);
                Ok(AddressOutcome::Committed)
            }
            AddressRoute::Demo => {
                self.attach(active, m2_demonstration_fixture())?;
                self.tabs[position].set_address(address);
                Ok(AddressOutcome::Committed)
            }
            AddressRoute::Unsupported => Ok(AddressOutcome::Rejected),
        }
    }

    /// Returns the active tab's committed address display string, if any.
    pub fn active_address_text(&self) -> Option<&str> {
        let active = self.active?;
        let tab = self
            .tabs
            .iter()
            .find(|candidate| candidate.id() == active)?;
        tab.address_text()
    }

    /// Makes a tab the active tab.
    ///
    /// Fails with `UnknownTab` when the identity names no tab.
    pub fn activate(&mut self, tab: TabId) -> Result<(), CoreError> {
        if !self.tabs.iter().any(|candidate| candidate.id() == tab) {
            return Err(CoreError::UnknownTab);
        }

        self.active = Some(tab);
        Ok(())
    }

    /// Closes a tab and detaches its document.
    ///
    /// Closing the active tab activates the next tab, else the previous tab, else
    /// no tab. Closing an unknown tab is a no-op.
    pub fn close_tab(&mut self, tab: TabId) {
        let Some(position) = self.tabs.iter().position(|candidate| candidate.id() == tab) else {
            return;
        };

        let removed = self.tabs.remove(position);
        if let TabContent::Attached(handle) = *removed.content() {
            self.session.detach(handle);
        }

        if self.active == Some(tab) {
            self.active = self.next_active(position);
        }
    }

    /// Produces the active tab's frame for the given viewport geometry.
    ///
    /// Returns `Ok(None)` when there is no active tab or the active tab is empty,
    /// so a consumer paints nothing rather than an empty frame. Runs synchronously
    /// on the caller's sequence.
    pub fn produce_active(
        &mut self,
        geometry: ViewportGeometry,
    ) -> Result<Option<DocumentFrame>, CoreError> {
        let Some(active) = self.active else {
            return Ok(None);
        };

        let Some(tab) = self.tabs.iter().find(|candidate| candidate.id() == active) else {
            return Ok(None);
        };

        let TabContent::Attached(handle) = *tab.content() else {
            return Ok(None);
        };

        let frame = self.session.produce(&handle, geometry)?;
        Ok(Some(frame))
    }

    /// Returns the active tab identity, if any.
    pub fn active_tab(&self) -> Option<TabId> {
        self.active
    }

    /// Returns the active attached document's generation, for a staleness guard.
    ///
    /// Returns `None` when there is no active tab or the active tab is empty.
    pub fn active_generation(&self) -> Option<DocumentGeneration> {
        let active = self.active?;
        let tab = self
            .tabs
            .iter()
            .find(|candidate| candidate.id() == active)?;
        match tab.content() {
            TabContent::Attached(handle) => Some(handle.generation()),
            TabContent::Empty => None,
        }
    }

    /// Returns the tabs in insertion order.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Selects the next active tab after removing the tab at `position`.
    ///
    /// The tab now at `position` is the next tab; if none, the previous tab; if
    /// none, no tab.
    fn next_active(&self, position: usize) -> Option<TabId> {
        if let Some(next) = self.tabs.get(position) {
            return Some(next.id());
        }

        if position > 0
            && let Some(previous) = self.tabs.get(position - 1)
        {
            return Some(previous.id());
        }

        None
    }
}

impl Default for TabModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use purr_embedding::{SeamError, m2_demonstration_fixture};
    use purr_graphics::Extent2d;

    fn geometry() -> ViewportGeometry {
        ViewportGeometry {
            content_extent: Extent2d::new(800, 600),
            device_pixel_ratio: 1.0,
        }
    }

    #[test]
    fn open_tab_mints_unique_ids_and_activates_the_new_tab() {
        let mut model = TabModel::new();

        let first = model.open_tab();
        let second = model.open_tab();

        assert_ne!(first, second);
        assert_eq!(model.active_tab(), Some(second));
    }

    #[test]
    fn open_tab_preserves_insertion_order() {
        let mut model = TabModel::new();

        let first = model.open_tab();
        let second = model.open_tab();
        let third = model.open_tab();

        let order: Vec<TabId> = model.tabs().iter().map(|tab| tab.id()).collect();
        assert_eq!(order, vec![first, second, third]);
    }

    #[test]
    fn attach_moves_a_tab_to_attached_and_produces_the_active_generation() {
        let mut model = TabModel::new();
        let tab = model.open_tab();

        model
            .attach(tab, m2_demonstration_fixture())
            .expect("attach succeeds");

        let generation = model.active_generation().expect("an active generation");
        let frame = model
            .produce_active(geometry())
            .expect("produce succeeds")
            .expect("an active frame");
        assert_eq!(frame.generation, generation);
    }

    #[test]
    fn attach_to_an_unknown_tab_reports_unknown_tab() {
        let mut model = TabModel::new();
        let tab = model.open_tab();
        model.close_tab(tab);

        assert_eq!(
            model.attach(tab, m2_demonstration_fixture()),
            Err(CoreError::UnknownTab)
        );
    }

    #[test]
    fn attach_above_the_source_bound_reports_the_mapped_seam_error() {
        let mut model = TabModel::new();
        let tab = model.open_tab();
        // The seam rejects a source above its bound; a buffer well over the engine
        // limit forces that failure without naming the engine constant.
        let source = vec![0u8; 9 * 1024 * 1024];

        assert_eq!(
            model.attach(tab, &source),
            Err(CoreError::Seam(SeamError::SourceTooLarge))
        );
    }

    #[test]
    fn reattach_detaches_the_old_document_and_produces_the_new_generation() {
        let mut model = TabModel::new();
        let tab = model.open_tab();

        model
            .attach(tab, m2_demonstration_fixture())
            .expect("first attach succeeds");
        let first_generation = model.active_generation().expect("a first generation");

        model
            .attach(tab, m2_demonstration_fixture())
            .expect("second attach succeeds");
        let second_generation = model.active_generation().expect("a second generation");

        assert_ne!(first_generation, second_generation);
        let frame = model
            .produce_active(geometry())
            .expect("produce succeeds")
            .expect("an active frame");
        assert_eq!(frame.generation, second_generation);
    }

    #[test]
    fn produce_active_returns_none_without_an_active_tab() {
        let mut model = TabModel::new();

        assert_eq!(model.produce_active(geometry()), Ok(None));
    }

    #[test]
    fn produce_active_returns_none_for_an_empty_active_tab() {
        let mut model = TabModel::new();
        model.open_tab();

        assert_eq!(model.produce_active(geometry()), Ok(None));
    }

    #[test]
    fn activate_sets_the_active_tab_and_rejects_an_unknown_tab() {
        let mut model = TabModel::new();
        let first = model.open_tab();
        let second = model.open_tab();

        model.activate(first).expect("activate succeeds");
        assert_eq!(model.active_tab(), Some(first));

        model.close_tab(second);
        assert_eq!(model.activate(second), Err(CoreError::UnknownTab));
    }

    #[test]
    fn close_active_tab_activates_the_next_then_the_previous_then_none() {
        let mut model = TabModel::new();
        let first = model.open_tab();
        let second = model.open_tab();
        let third = model.open_tab();

        model.activate(second).expect("activate succeeds");
        model.close_tab(second);
        assert_eq!(model.active_tab(), Some(third));

        model.close_tab(third);
        assert_eq!(model.active_tab(), Some(first));

        model.close_tab(first);
        assert_eq!(model.active_tab(), None);
    }

    #[test]
    fn close_tab_detaches_the_document() {
        let mut model = TabModel::new();
        let tab = model.open_tab();
        model
            .attach(tab, m2_demonstration_fixture())
            .expect("attach succeeds");

        model.close_tab(tab);

        assert_eq!(model.active_tab(), None);
        assert_eq!(model.produce_active(geometry()), Ok(None));
    }

    #[test]
    fn submit_demo_attaches_the_fixture_and_commits_the_address() {
        let mut model = TabModel::new();
        model.open_tab();

        assert_eq!(
            model.submit_address("panther:demo"),
            Ok(AddressOutcome::Committed)
        );
        assert_eq!(model.active_address_text(), Some("panther:demo"));
        assert!(
            model
                .produce_active(geometry())
                .expect("produce succeeds")
                .is_some()
        );
    }

    #[test]
    fn submit_blank_detaches_content_and_commits_the_address() {
        let mut model = TabModel::new();
        let tab = model.open_tab();
        model
            .attach(tab, m2_demonstration_fixture())
            .expect("attach succeeds");

        assert_eq!(
            model.submit_address("panther:blank"),
            Ok(AddressOutcome::Committed)
        );
        assert_eq!(model.active_address_text(), Some("panther:blank"));
        assert_eq!(model.produce_active(geometry()), Ok(None));
    }

    #[test]
    fn submit_rejects_unparseable_and_unsupported_input_uniformly() {
        let mut model = TabModel::new();
        model.open_tab();

        let unparseable = model.submit_address("not a url");
        let unsupported = model.submit_address("https://example.com");

        assert_eq!(unparseable, Ok(AddressOutcome::Rejected));
        assert_eq!(unsupported, Ok(AddressOutcome::Rejected));
        assert_eq!(model.active_address_text(), None);
    }

    #[test]
    fn submit_without_an_active_tab_is_rejected() {
        let mut model = TabModel::new();

        assert_eq!(
            model.submit_address("panther:demo"),
            Ok(AddressOutcome::Rejected)
        );
    }

    #[test]
    fn a_fresh_tab_reports_no_committed_address() {
        let mut model = TabModel::new();
        model.open_tab();

        assert_eq!(model.active_address_text(), None);
    }
}
