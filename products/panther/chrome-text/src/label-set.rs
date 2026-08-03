// @file products/panther/chrome-text/src/label-set.rs
// @description Maps chrome regions to catalogue keys and resolves their labels.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::fmt;

use panther_localization::{
    ActiveLocaleState, LocaleGeneration, MessageCatalog, neutralize_bidi_controls,
};
use panther_shell::ShellRegion;

/// The chrome regions that carry a label and the catalogue key for each.
///
/// The table is the presentation boundary: it holds typed keys, never prose.
/// Only these regions carry text; the other regions stay colored rectangles. A
/// new labeled region requires a new entry, which keeps the map explicit.
const LABELED_REGIONS: [(ShellRegion, &str); 4] = [
    (ShellRegion::NavigationBack, "toolbar-back"),
    (ShellRegion::NavigationForward, "toolbar-forward"),
    (ShellRegion::NavigationReload, "toolbar-reload"),
    (ShellRegion::AddressField, "address-placeholder"),
];

/// The resolved chrome labels for one active-locale generation.
///
/// The value is internal to the producer: it carries the translated strings so
/// a later phase can shape them, but it exposes no string across a crate
/// boundary, so the prose never reaches the prose-free shell. It records the
/// generation it was resolved under so a later cache drops it once the
/// generation advances.
pub struct ChromeLabels {
    entries: [(ShellRegion, String); 4],
    generation: LocaleGeneration,
}

impl ChromeLabels {
    /// Returns the active-locale generation the labels were resolved under.
    pub fn generation(&self) -> LocaleGeneration {
        self.generation
    }

    /// The resolved region-to-string entries.
    ///
    /// The accessor stays crate-internal: the producer shapes these strings and
    /// exposes only the neutral glyph geometry, so the prose never crosses the
    /// crate boundary into the prose-free shell. It also lets the producer cache
    /// compare the resolved label set to decide whether a rebuild is required.
    pub(crate) fn entries(&self) -> &[(ShellRegion, String); 4] {
        &self.entries
    }
}

impl fmt::Debug for ChromeLabels {
    /// Redacts the resolved prose so a diagnostic never leaks a translated
    /// label. It reports the generation and the labeled regions only, never the
    /// text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChromeLabels")
            .field("generation", &self.generation)
            .field(
                "regions",
                &self.entries.each_ref().map(|(region, _)| *region),
            )
            .finish()
    }
}

/// Resolves every chrome label at the active locale and generation.
///
/// Each key resolves through the catalogue in the active user-interface locale
/// and falls back to `en` when that locale ships no catalogue. Displayed text
/// runs through bidi neutralization so a directional control can never reorder a
/// label; the current labels are trusted static prose, but keeping the call on
/// the path protects a future dynamic label such as a URL.
pub fn resolve_labels(state: &ActiveLocaleState, catalog: &MessageCatalog) -> ChromeLabels {
    let generation = state.generation();
    let ui_locale = state.current().ui_locale().clone();
    let localized = catalog.at_generation(generation).for_locales(&[ui_locale]);

    let entries = LABELED_REGIONS.map(|(region, key)| {
        let message = localized.message(key);
        let text = neutralize_bidi_controls(message.text()).into_owned();
        (region, text)
    });

    ChromeLabels {
        entries,
        generation,
    }
}

#[cfg(test)]
mod tests {
    use locale::Locale;
    use panther_localization::{ActiveLocaleState, LocaleRequest, LocaleResolver, MessageCatalog};
    use panther_shell::ShellRegion;

    use super::{ChromeLabels, resolve_labels};

    fn locale(identifier: &str) -> Locale {
        Locale::parse(identifier).expect("valid identifier")
    }

    fn resolver() -> LocaleResolver {
        LocaleResolver::new(
            vec![locale("en"), locale("es"), locale("de")],
            vec![locale("en")],
        )
    }

    fn label(labels: &ChromeLabels, region: ShellRegion) -> Option<&str> {
        labels
            .entries
            .iter()
            .find(|(candidate, _)| *candidate == region)
            .map(|(_, text)| text.as_str())
    }

    #[test]
    fn english_locale_resolves_the_english_labels() {
        let state = ActiveLocaleState::new(resolver(), LocaleRequest::new());
        let catalog = MessageCatalog::load();

        let labels = resolve_labels(&state, &catalog);

        assert_eq!(label(&labels, ShellRegion::NavigationBack), Some("Back"));
        assert_eq!(
            label(&labels, ShellRegion::NavigationForward),
            Some("Forward")
        );
        assert_eq!(
            label(&labels, ShellRegion::NavigationReload),
            Some("Reload")
        );
        assert_eq!(
            label(&labels, ShellRegion::AddressField),
            Some("Search or enter address")
        );
    }

    #[test]
    fn spanish_locale_resolves_the_spanish_labels() {
        let mut state = ActiveLocaleState::new(resolver(), LocaleRequest::new());
        state.change_language(locale("es"));
        let catalog = MessageCatalog::load();

        let labels = resolve_labels(&state, &catalog);

        assert_eq!(label(&labels, ShellRegion::NavigationBack), Some("Atrás"));
        assert_eq!(
            label(&labels, ShellRegion::NavigationForward),
            Some("Adelante")
        );
        assert_eq!(
            label(&labels, ShellRegion::NavigationReload),
            Some("Recargar")
        );
        assert_eq!(
            label(&labels, ShellRegion::AddressField),
            Some("Buscar o escribir dirección")
        );
    }

    #[test]
    fn unavailable_locale_falls_back_to_english() {
        let mut state = ActiveLocaleState::new(resolver(), LocaleRequest::new());
        state.change_language(locale("de"));
        let catalog = MessageCatalog::load();

        let labels = resolve_labels(&state, &catalog);

        assert_eq!(state.current().ui_locale().to_string(), "de");
        assert_eq!(label(&labels, ShellRegion::NavigationBack), Some("Back"));
        assert_eq!(
            label(&labels, ShellRegion::AddressField),
            Some("Search or enter address")
        );
    }

    #[test]
    fn labels_carry_the_active_generation() {
        let mut state = ActiveLocaleState::new(resolver(), LocaleRequest::new());
        assert_eq!(state.generation().value(), 1);
        state.change_language(locale("es"));
        let catalog = MessageCatalog::load();

        let labels = resolve_labels(&state, &catalog);

        assert_eq!(labels.generation(), state.generation());
        assert_eq!(labels.generation().value(), 2);
    }
}
