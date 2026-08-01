// @file products/panther/localization/src/locale-broadcast.rs
// @description Notifies subscribers when the active-locale generation advances.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::rc::{Rc, Weak};

use crate::locale_generation::LocaleGeneration;

/// A component that reacts to a runtime language change.
///
/// A subscriber re-pulls its text after a change rather than receiving the new
/// prose through the notification, so the broadcast carries only the new
/// generation. Platform-native surfaces are rebuilt on change, not mutated in
/// place, and accessibility must re-announce the change; those surfaces arrive
/// in a later milestone and are not implemented here.
pub trait LocaleChangeListener {
    /// Reacts to the active-locale generation advancing.
    fn on_locale_change(&self, generation: LocaleGeneration);
}

/// Notifies subscribers when the active locale changes.
///
/// The broadcast holds a weak reference to each subscriber, so it never keeps a
/// subscriber alive and ownership stays with the subscriber. A notification is
/// synchronous, so there is no detached task and no unbounded queue. A
/// subscriber that has been dropped is removed during the next notification, so
/// the subscriber list cannot grow without bound.
pub struct LocaleBroadcast {
    listeners: Vec<Weak<dyn LocaleChangeListener>>,
}

impl LocaleBroadcast {
    /// Creates a broadcast with no subscribers.
    pub fn new() -> Self {
        Self {
            listeners: Vec::new(),
        }
    }

    /// Registers a subscriber for later change notifications.
    pub fn subscribe(&mut self, listener: &Rc<dyn LocaleChangeListener>) {
        self.listeners.push(Rc::downgrade(listener));
    }

    /// Notifies every live subscriber that the generation advanced.
    pub fn broadcast(&mut self, generation: LocaleGeneration) {
        self.listeners.retain(|listener| match listener.upgrade() {
            Some(listener) => {
                listener.on_locale_change(generation);
                true
            }
            None => false,
        });
    }
}

impl Default for LocaleBroadcast {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{LocaleBroadcast, LocaleChangeListener};
    use crate::locale_generation::LocaleGeneration;

    #[derive(Default)]
    struct RecordingListener {
        seen: RefCell<Vec<LocaleGeneration>>,
    }

    impl LocaleChangeListener for RecordingListener {
        fn on_locale_change(&self, generation: LocaleGeneration) {
            self.seen.borrow_mut().push(generation);
        }
    }

    #[test]
    fn live_subscriber_receives_the_new_generation() {
        let mut broadcast = LocaleBroadcast::new();
        let listener = Rc::new(RecordingListener::default());
        broadcast.subscribe(&(listener.clone() as Rc<dyn LocaleChangeListener>));

        broadcast.broadcast(LocaleGeneration::new(2));

        assert_eq!(
            listener.seen.borrow().as_slice(),
            &[LocaleGeneration::new(2)]
        );
    }

    #[test]
    fn dropped_subscriber_is_removed_without_panic() {
        let mut broadcast = LocaleBroadcast::new();
        let listener = Rc::new(RecordingListener::default());
        broadcast.subscribe(&(listener.clone() as Rc<dyn LocaleChangeListener>));
        drop(listener);

        broadcast.broadcast(LocaleGeneration::new(2));

        assert!(broadcast.listeners.is_empty());
    }
}
