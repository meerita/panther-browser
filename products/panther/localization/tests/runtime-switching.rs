// @file products/panther/localization/tests/runtime-switching.rs
// @description Integration test for a runtime switch and generation invalidation.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use locale::Locale;
use panther_localization::{
    ActiveLocaleState, FormatterCache, LocaleChangeListener, LocaleGeneration, LocaleRequest,
    LocaleResolver, MessageCatalog,
};

/// Records every generation a runtime language change broadcasts.
#[derive(Default)]
struct GenerationRecorder {
    seen: RefCell<Vec<u64>>,
}

impl LocaleChangeListener for GenerationRecorder {
    fn on_locale_change(&self, generation: LocaleGeneration) {
        self.seen.borrow_mut().push(generation.value());
    }
}

fn locale(identifier: &str) -> Locale {
    Locale::parse(identifier).expect("valid identifier")
}

/// A runtime switch advances the generation and invalidates cached values (D7).
///
/// The switch re-resolves the active locale, advances the generation, and
/// notifies subscribers. A previously produced message and a previously cached
/// formatter belong to the earlier generation, so a generation-aware cache drops
/// them. The assertions read the generation value and the formatter identity, not
/// any translated text.
#[test]
fn runtime_switch_invalidates_the_previous_generation() {
    let resolver = LocaleResolver::new(
        vec![locale("en"), locale("es"), locale("ja")],
        vec![locale("en")],
    );
    let mut state = ActiveLocaleState::new(resolver, LocaleRequest::new());
    assert_eq!(state.generation().value(), 1);
    assert_eq!(state.current().ui_locale().to_string(), "en");

    let recorder = Rc::new(GenerationRecorder::default());
    state.subscribe(&(recorder.clone() as Rc<dyn LocaleChangeListener>));

    let catalog = MessageCatalog::load();
    let before = catalog.at_generation(state.generation()).window_title();
    assert_eq!(before.generation().value(), 1);

    let cache = FormatterCache::new();
    let region = state.current().region_locale().clone();
    let formatter_before = cache.get(&region, state.generation());

    state.change_language(locale("es"));

    // The switch re-resolves and advances the generation.
    assert_eq!(state.generation().value(), 2);
    assert_eq!(state.current().ui_locale().to_string(), "es");

    // The broadcast delivered exactly the new generation to the subscriber.
    assert_eq!(recorder.seen.borrow().as_slice(), &[2]);

    // A value produced after the switch carries the new generation, so a
    // generation-aware cache never serves the earlier value.
    let after = catalog.at_generation(state.generation()).window_title();
    assert_eq!(after.generation().value(), 2);
    assert_ne!(before.generation(), after.generation());

    // The formatter cache rebuilds across the generation boundary rather than
    // serving the instance from the earlier generation.
    let formatter_after = cache.get(&region, state.generation());
    assert!(!Arc::ptr_eq(&formatter_before, &formatter_after));
}
