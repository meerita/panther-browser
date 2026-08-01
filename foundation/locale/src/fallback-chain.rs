// @file foundation/locale/src/fallback-chain.rs
// @description Produces the data-fallback chain for a locale.
// @created Diego Martín Lafuente <meerita@icloud.com>

use icu_locale::LocaleFallbacker;

use crate::locale::Locale;

/// Produces the data-fallback chain for a locale, most specific first.
///
/// The chain follows the ICU4X locale fallback algorithm and ends with the
/// unknown locale `und`, so a regional locale such as `es-AR` yields `es-AR`,
/// `es-419`, `es`, `und`. Dependent crates walk the chain to look up per-message
/// data from the most specific locale down to the ultimate fallback.
pub fn fallback_chain(locale: &Locale) -> Vec<Locale> {
    let fallbacker = LocaleFallbacker::new();
    let mut iterator = fallbacker
        .for_config(Default::default())
        .fallback_for(locale.as_icu().into());

    let mut chain = Vec::new();
    loop {
        let current = *iterator.get();
        chain.push(Locale::from_icu(current.into_locale()));
        if current.is_unknown() {
            break;
        }
        iterator.step();
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::fallback_chain;
    use crate::locale::Locale;

    #[test]
    fn regional_locale_falls_back_through_macroregion() {
        let locale = Locale::parse("es-AR").expect("valid identifier");

        let chain: Vec<String> = fallback_chain(&locale)
            .iter()
            .map(|entry| entry.to_string())
            .collect();

        assert_eq!(chain, ["es-AR", "es-419", "es", "und"]);
    }
}
