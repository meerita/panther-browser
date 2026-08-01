// @file products/panther/localization/src/locale-generation.rs
// @description Identifies the active-locale generation a message was produced under.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// The active-locale generation a localized value was produced under.
///
/// Runtime language switching advances a single active-locale generation
/// counter. A [`LocalizedMessage`] records the generation it was produced under
/// so that generation-aware caches drop any value that crosses a generation
/// boundary. The counter itself is owned by the resolution service in a later
/// phase; this type only carries the value. Real generations start at one, so
/// they never collide with [`LocaleGeneration::DETACHED`].
///
/// [`LocalizedMessage`]: crate::LocalizedMessage
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LocaleGeneration(u64);

impl LocaleGeneration {
    /// The generation of text that is not tied to a resolved locale.
    ///
    /// The escape hatch produces messages from text that never went through
    /// locale resolution, so those messages carry this detached generation and
    /// never match a real generation boundary.
    pub const DETACHED: Self = Self(0);

    /// Wraps a raw generation value produced by the resolution service.
    ///
    /// The resolution service that consumes this is added in a later phase.
    #[allow(dead_code)]
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw generation value.
    pub const fn value(self) -> u64 {
        self.0
    }
}
