// @file products/panther/localization/src/formatter-cache.rs
// @description Memoizes regional formatters keyed by region locale.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use locale::Locale;

use crate::regional_formatter::RegionalFormatter;

/// The reference locale identifier and formatter fallback.
const REFERENCE: &str = "en";

/// Caches one [`RegionalFormatter`] per region locale.
///
/// A formatter is built the first time a region is requested and shared behind
/// an [`Arc`] on every later request, so a formatter is never rebuilt per call.
/// A region without formatting data reuses the reference formatter, so a lookup
/// always returns a usable formatter and never fails.
pub struct FormatterCache {
    reference: Arc<RegionalFormatter>,
    formatters: Mutex<HashMap<Locale, Arc<RegionalFormatter>>>,
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
            formatters: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the shared formatter for a region locale.
    pub fn get(&self, region: &Locale) -> Arc<RegionalFormatter> {
        let mut formatters = self
            .formatters
            .lock()
            .expect("the formatter cache lock is not poisoned");
        if let Some(existing) = formatters.get(region) {
            return existing.clone();
        }
        let formatter = RegionalFormatter::new(region)
            .map(Arc::new)
            .unwrap_or_else(|_| self.reference.clone());
        formatters.insert(region.clone(), formatter.clone());
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
    use locale::Locale;
    use std::sync::Arc;

    #[test]
    fn repeated_lookup_reuses_the_same_instance() {
        let cache = FormatterCache::new();
        let region = Locale::parse("en-US").expect("valid identifier");
        let first = cache.get(&region);
        let second = cache.get(&region);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn distinct_regions_get_distinct_instances() {
        let cache = FormatterCache::new();
        let english = cache.get(&Locale::parse("en-US").expect("valid identifier"));
        let german = cache.get(&Locale::parse("de-DE").expect("valid identifier"));
        assert!(!Arc::ptr_eq(&english, &german));
    }
}
