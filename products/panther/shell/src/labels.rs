// @file products/panther/shell/src/labels.rs
// @description Defines the neutral label view that maps a chrome region to its placed glyph run.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_text::PlacedGlyphRun;

use crate::shell_region::ShellRegion;

/// The neutral chrome text the shell paints: one placed glyph run per region.
///
/// The window rebuilds the view from the chrome text producer and hands it in on
/// every locale or label change. The view carries only neutral geometry and the
/// atlas identity of each run, never a string or a localized message, so the
/// shell stays prose-free. It owns its runs and compares by value, so the shell
/// tracks a change to the rendered chrome without holding the atlas or a shaped
/// run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LabelView {
    runs: Vec<(ShellRegion, PlacedGlyphRun)>,
}

impl LabelView {
    /// Builds a view from the placed run of each labeled region.
    pub fn new(runs: Vec<(ShellRegion, PlacedGlyphRun)>) -> Self {
        Self { runs }
    }

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

    /// Whether the view carries no run.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}
