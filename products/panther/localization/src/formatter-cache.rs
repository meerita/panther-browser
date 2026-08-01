// @file products/panther/localization/src/formatter-cache.rs
// @description Memoizes regional formatters keyed by region locale.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use locale::Locale;

use crate::locale_generation::LocaleGeneration;
use crate::regional_formatter::RegionalFormatter;

/// The reference locale identifier and formatter fallback.
const REFERENCE: &str = "en";

/// The generation-tagged formatter map.
///
/// The generation guards the map so that a formatter built under an earlier
/// active locale can never be served after a runtime language change.
struct CacheState {
    generation: LocaleGeneration,
    formatters: HashMap<Locale, Arc<RegionalFormatter>>,
}

/// Caches one [`RegionalFormatter`] per region locale for one generation.
///
/// A formatter is built the first time a region is requested and shared behind
/// an [`Arc`] on every later request in the same generation, so a formatter is
/// never rebuilt per call. When the active-locale generation advances the cache
/// clears, so no formatter is served across a generation boundary. A region
/// without formatting data reuses the reference formatter, so a lookup always
/// returns a usable formatter and never fails.
pub struct FormatterCache {
    reference: Arc<RegionalFormatter>,
    state: Mutex<CacheState>,
}

impl FormatterCache {
    /// Creates a cache with the reference formatter ready as the fallback.
    pub fn new() -> Self {
        let reference_locale = Locale::parse(REFERENCE).expect("the reference locale is valid");
        let reference = RegionalFormatter::new(&reference_locale)
            .map(Arc::new)
            .expect("the reference locale has compiled formatting data");
        Self {
            reference,
            state: Mutex::new(CacheState {
                generation: LocaleGeneration::FIRST,
                formatters: HashMap::new(),
            }),
        }
    }

    /// Returns the shared formatter for a region locale at a generation.
    ///
    /// A generation later than the cached one clears the cache first, so a
    /// formatter from a previous active locale is never returned.
    pub fn get(&self, region: &Locale, generation: LocaleGeneration) -> Arc<RegionalFormatter> {
        let mut state = self
            .state
            .lock()
            .expect("the formatter cache lock is not poisoned");
        if state.generation != generation {
            state.formatters.clear();
            state.generation = generation;
        }
        if let Some(existing) = state.formatters.get(region) {
            return existing.clone();
        }
        let formatter = RegionalFormatter::new(region)
            .map(Arc::new)
            .unwrap_or_else(|_| self.reference.clone());
        state.formatters.insert(region.clone(), formatter.clone());
        formatter
    }
}

impl Default for FormatterCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::FormatterCache;
    use crate::locale_generation::LocaleGeneration;
    use locale::Locale;
    use std::sync::Arc;

    #[test]
    fn repeated_lookup_reuses_the_same_instance() {
        let cache = FormatterCache::new();
        let region = Locale::parse("en-US").expect("valid identifier");
        let first = cache.get(&region, LocaleGeneration::FIRST);
        let second = cache.get(&region, LocaleGeneration::FIRST);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn distinct_regions_get_distinct_instances() {
        let cache = FormatterCache::new();
        let generation = LocaleGeneration::FIRST;
        let english = cache.get(
            &Locale::parse("en-US").expect("valid identifier"),
            generation,
        );
        let german = cache.get(
            &Locale::parse("de-DE").expect("valid identifier"),
            generation,
        );
        assert!(!Arc::ptr_eq(&english, &german));
    }

    #[test]
    fn advancing_the_generation_invalidates_a_cached_formatter() {
        let cache = FormatterCache::new();
        let region = Locale::parse("en-US").expect("valid identifier");
        let first = cache.get(&region, LocaleGeneration::new(1));
        let rebuilt = cache.get(&region, LocaleGeneration::new(2));
        assert!(!Arc::ptr_eq(&first, &rebuilt));
    }
}
