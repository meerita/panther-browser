// @file products/panther/chrome-text/src/producer.rs
// @description Shapes the resolved chrome labels, builds one chrome glyph atlas, and caches the placed runs by locale generation.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Chrome text producer.
//!
//! The producer is the sole translator and shaper for the shell chrome. It owns
//! the bundled font, the active locale state, and the message catalog, so no
//! prose leaves the crate: it resolves the four region labels, shapes each into a
//! glyph run, packs every label glyph into one atlas, and combines each run with
//! that atlas into a neutral [`PlacedGlyphRun`]. The shell reads the placed runs;
//! the window uploads the atlas.
//!
//! The atlas lives in a chrome producer namespace distinct from the engine
//! namespace, so the chrome atlas identity can never collide with a document
//! atlas identity. Each rebuild advances the atlas resource generation, so a
//! rebuilt atlas gets a fresh identity and the window re-realizes it.
//!
//! Building an atlas rasterizes glyphs, so it is not per-frame work. The producer
//! stamps every built result with the locale generation it was built under and
//! rebuilds only when the generation advances or the resolved labels change. It
//! never serves a result stamped with a generation other than the active one.

use locale::Locale;
use panther_localization::{ActiveLocaleState, LocaleGeneration, MessageCatalog};
use panther_shell::ShellRegion;
use purr_graphics::{
    DeviceGeneration, GpuResourceIdentity, ProducerNamespace, ResourceGeneration, ResourceId,
    ResourceUpload,
};
use purr_text::{
    BundledFont, CmapOneToOneAdapter, FontError, GlyphAtlasError, GlyphKey, GlyphRun,
    GlyphRunGeneration, GlyphRunId, ONE_PX_RAW, PlacedGlyphRun, ShapingError, ShapingRequest,
    TextShapingAdapter, TextUnit,
};

use crate::label_set::{ChromeLabels, resolve_labels};

/// The fixed chrome text size, in device pixels.
///
/// One size fits the toolbar control height for this iteration; the chrome uses
/// no other text size, so the atlas packs one mask per glyph. The value is a
/// raw fixed-point unit, so it needs no fallible construction.
const CHROME_FONT_SIZE: TextUnit = TextUnit::from_raw(15 * ONE_PX_RAW);

/// The producer namespace for the chrome atlas.
///
/// It is distinct from the engine namespace (`2`), so the chrome atlas identity
/// never collides with a document atlas identity even at the same resource id.
const CHROME_NAMESPACE: u32 = 3;

/// The resource id of the single chrome glyph atlas.
const CHROME_ATLAS_RESOURCE_ID: u64 = 1;

/// The device generation the chrome atlas is stamped with.
///
/// It stays stable across rebuilds: only the resource generation advances, so a
/// rebuild changes the resource identity without implying a device loss.
const CHROME_DEVICE_GENERATION: u64 = 1;

/// The first atlas resource generation. Each rebuild advances it.
const FIRST_RESOURCE_GENERATION: u64 = 1;

/// A failure while the producer builds the chrome text.
#[derive(Debug, thiserror::Error)]
pub enum ChromeTextError {
    /// The bundled chrome font could not be loaded.
    #[error("the bundled chrome font could not be loaded")]
    Font(#[from] FontError),
    /// A chrome label could not be shaped.
    #[error("a chrome label could not be shaped")]
    Shaping(#[from] ShapingError),
    /// The chrome glyph atlas could not be built.
    #[error("the chrome glyph atlas could not be built")]
    Atlas(#[from] GlyphAtlasError),
    /// The chrome font metrics do not scale into range at the chrome size.
    #[error("the chrome font metrics are out of range")]
    Metrics,
}

/// The neutral chrome text the shell paints: one placed glyph run per region.
///
/// The view owns its runs and compares by value, so the shell tracks a change to
/// the rendered chrome without holding the atlas or a shaped run. It carries no
/// prose: a [`PlacedGlyphRun`] holds glyph geometry and an atlas identity only.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeTextView {
    runs: Vec<(ShellRegion, PlacedGlyphRun)>,
}

impl ChromeTextView {
    /// The placed run for each labeled region, in region order.
    pub fn runs(&self) -> &[(ShellRegion, PlacedGlyphRun)] {
        &self.runs
    }

    /// The placed run for one region, or `None` when the region carries no label.
    pub fn run(&self, region: ShellRegion) -> Option<&PlacedGlyphRun> {
        self.runs
            .iter()
            .find(|(candidate, _)| *candidate == region)
            .map(|(_, run)| run)
    }
}

/// One built chrome atlas: the shell view, the atlas upload, and the inputs that
/// produced it.
///
/// The labels and the generation are the cache key: the producer keeps a built
/// result while the active generation and the resolved labels match, and rebuilds
/// otherwise. The stored generation is always the active generation of the served
/// result, so a served view is never stamped with a stale generation.
struct ChromeBuild {
    generation: LocaleGeneration,
    labels: [(ShellRegion, String); 4],
    view: ChromeTextView,
    upload: ResourceUpload,
}

/// The chrome text producer.
///
/// It owns the font, the locale state, and the catalog, and holds one built
/// atlas. [`ChromeText::refresh`] rebuilds only on a locale-generation or label
/// change; the accessors expose the shell view and the window upload.
pub struct ChromeText {
    font: BundledFont,
    state: ActiveLocaleState,
    catalog: MessageCatalog,
    resource_generation: u64,
    build: ChromeBuild,
}

impl ChromeText {
    /// Loads the bundled font and builds the first chrome atlas at the active
    /// locale.
    ///
    /// It resolves the four labels, shapes each at the chrome size, packs every
    /// label glyph into one atlas in the chrome namespace, and combines each run
    /// with the atlas into a placed run. It fails closed with a typed error when
    /// the font, shaping, or atlas build fails.
    pub fn new(state: ActiveLocaleState, catalog: MessageCatalog) -> Result<Self, ChromeTextError> {
        let font = BundledFont::load()?;
        let labels = resolve_labels(&state, &catalog);
        let resource_generation = FIRST_RESOURCE_GENERATION;
        let build = build_atlas(&font, &labels, resource_generation)?;

        Ok(Self {
            font,
            state,
            catalog,
            resource_generation,
            build,
        })
    }

    /// Rebuilds the chrome atlas when the locale generation advanced or the
    /// resolved labels changed; otherwise keeps the built result.
    ///
    /// It first checks the active generation against the built generation; while
    /// they match, nothing changed and it returns without shaping. When the
    /// generation advanced it re-resolves the labels: if the resolved set is
    /// unchanged it only re-stamps the built result with the active generation, so
    /// the atlas identity stays stable and the window need not re-realize;
    /// otherwise it advances the resource generation and builds a fresh atlas with
    /// a new identity. It fails closed with a typed error when the rebuild fails.
    pub fn refresh(&mut self) -> Result<(), ChromeTextError> {
        let generation = self.state.generation();
        if self.build.generation == generation {
            return Ok(());
        }

        let labels = resolve_labels(&self.state, &self.catalog);
        if *labels.entries() == self.build.labels {
            self.build.generation = generation;
            return Ok(());
        }

        self.resource_generation += 1;
        self.build = build_atlas(&self.font, &labels, self.resource_generation)?;
        Ok(())
    }

    /// Changes the active user-interface language and advances the locale
    /// generation.
    ///
    /// The producer owns the locale state, so this is the seam a language switch
    /// drives. It only advances the generation; the next [`ChromeText::refresh`]
    /// rebuilds when the resolved labels changed.
    pub fn change_language(&mut self, language: Locale) {
        self.state.change_language(language);
    }

    /// The neutral chrome text view for the shell.
    pub fn view(&self) -> &ChromeTextView {
        &self.build.view
    }

    /// The atlas resource upload for the window to realize.
    pub fn upload(&self) -> &ResourceUpload {
        &self.build.upload
    }

    /// The identity of the chrome atlas the placed runs sample.
    pub fn identity(&self) -> GpuResourceIdentity {
        self.build.upload.resource
    }
}

/// Shapes every resolved label, packs one atlas, and builds a placed run per
/// region.
///
/// Every label shapes at the chrome size, so one atlas over the union of the
/// label glyphs serves all runs. The atlas takes the chrome namespace, the chrome
/// resource id, the given resource generation, and the stable device generation,
/// so a fresh resource generation yields a fresh identity.
fn build_atlas(
    font: &BundledFont,
    labels: &ChromeLabels,
    resource_generation: u64,
) -> Result<ChromeBuild, ChromeTextError> {
    let mut runs: Vec<(ShellRegion, GlyphRun)> = Vec::with_capacity(labels.entries().len());
    for (index, (region, text)) in labels.entries().iter().enumerate() {
        let run = CmapOneToOneAdapter.shape(ShapingRequest {
            font,
            text,
            size: CHROME_FONT_SIZE,
            run_id: GlyphRunId::new(index as u32 + 1),
            generation: GlyphRunGeneration::new(1),
        })?;
        runs.push((*region, run));
    }

    let mut keys: Vec<GlyphKey> = Vec::new();
    for (_, run) in &runs {
        for positioned in run.glyphs() {
            keys.push(GlyphKey::new(positioned.glyph(), CHROME_FONT_SIZE));
        }
    }

    let atlas = purr_text::build_glyph_atlas(
        font,
        &keys,
        ProducerNamespace::new(CHROME_NAMESPACE),
        ResourceId::new(CHROME_ATLAS_RESOURCE_ID),
        ResourceGeneration::new(resource_generation),
        DeviceGeneration::new(CHROME_DEVICE_GENERATION),
    )?;
    let metrics = font
        .metrics(CHROME_FONT_SIZE)
        .ok_or(ChromeTextError::Metrics)?;

    let view = ChromeTextView {
        runs: runs
            .iter()
            .map(|(region, run)| {
                (
                    *region,
                    PlacedGlyphRun::from_shaped_run(run, &atlas, metrics),
                )
            })
            .collect(),
    };

    Ok(ChromeBuild {
        generation: labels.generation(),
        labels: labels.entries().clone(),
        view,
        upload: atlas.upload().clone(),
    })
}

#[cfg(test)]
mod tests {
    use locale::Locale;
    use panther_localization::{ActiveLocaleState, LocaleRequest, LocaleResolver, MessageCatalog};
    use panther_shell::ShellRegion;

    use super::{CHROME_NAMESPACE, ChromeText};

    /// The engine document atlas namespace the chrome atlas must never collide
    /// with (`engine_producer_namespace()` is `ProducerNamespace::new(2)`).
    const ENGINE_NAMESPACE: u32 = 2;

    fn locale(identifier: &str) -> Locale {
        Locale::parse(identifier).expect("valid identifier")
    }

    fn resolver() -> LocaleResolver {
        LocaleResolver::new(
            vec![locale("en"), locale("es"), locale("de")],
            vec![locale("en")],
        )
    }

    fn producer() -> ChromeText {
        let state = ActiveLocaleState::new(resolver(), LocaleRequest::new());
        ChromeText::new(state, MessageCatalog::load()).expect("the producer builds")
    }

    #[test]
    fn a_locale_yields_a_view_and_exactly_one_atlas_upload() {
        let producer = producer();

        let regions: Vec<ShellRegion> = producer
            .view()
            .runs()
            .iter()
            .map(|(region, _)| *region)
            .collect();
        assert_eq!(
            regions,
            [
                ShellRegion::NavigationBack,
                ShellRegion::NavigationForward,
                ShellRegion::NavigationReload,
                ShellRegion::AddressField,
            ]
        );

        // Every region carries a placed run, and every run samples the one atlas
        // the producer exposes, so the whole chrome uploads a single atlas.
        let atlas = producer.identity();
        assert_eq!(producer.upload().resource, atlas);
        for (_, run) in producer.view().runs() {
            assert_eq!(run.atlas(), atlas);
        }
        assert!(producer.view().run(ShellRegion::AddressField).is_some());
        assert!(producer.view().run(ShellRegion::TopBar).is_none());
    }

    #[test]
    fn the_atlas_identity_uses_the_chrome_namespace_distinct_from_the_engine() {
        let producer = producer();

        assert_eq!(
            producer.identity().producer_namespace().value(),
            CHROME_NAMESPACE
        );
        assert_ne!(CHROME_NAMESPACE, ENGINE_NAMESPACE);
        assert_ne!(
            producer.identity().producer_namespace().value(),
            ENGINE_NAMESPACE
        );
    }

    #[test]
    fn the_cache_does_not_rebuild_when_nothing_changed() {
        let mut producer = producer();
        let identity = producer.identity();
        let view = producer.view().clone();

        producer.refresh().expect("refresh with no change succeeds");

        // The identity and the view are byte-for-byte unchanged, so no atlas was
        // rasterized again.
        assert_eq!(producer.identity(), identity);
        assert_eq!(producer.view(), &view);
        assert_eq!(
            producer.identity().resource_generation(),
            identity.resource_generation()
        );
    }

    #[test]
    fn the_cache_rebuilds_when_the_locale_generation_advances() {
        let mut producer = producer();
        let before = producer.identity();
        let english = producer.view().clone();

        producer.change_language(locale("es"));
        producer.refresh().expect("the rebuild succeeds");

        // A new locale changes the labels, so the atlas is rebuilt: the resource
        // generation advances and the identity changes.
        let after = producer.identity();
        assert_ne!(after, before);
        assert!(after.resource_generation().value() > before.resource_generation().value());
        // The view changed with the new language, so the shell repaints.
        assert_ne!(producer.view(), &english);
    }

    #[test]
    fn an_unavailable_locale_falls_back_and_keeps_the_english_view() {
        // The resolver ships only `en` and `es`; a request for `de` resolves the
        // labels through the `en` fallback, so the view matches the English one.
        let english = producer();
        let english_view = english.view().clone();

        let mut fallback = producer();
        fallback.change_language(locale("de"));
        fallback.refresh().expect("the refresh succeeds");

        assert_eq!(fallback.view(), &english_view);
    }
}
