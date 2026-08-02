// @file foundation/memory/src/region.rs
// @description Defines the eight memory regions of the engine.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// A memory region of the engine.
///
/// The engine divides process memory into eight regions, each with its own
/// allocation strategy. The short-lived regions (`Document`, `Parser`, `Frame`)
/// take arena allocation; the capacity regions (`Images`, `Fonts`, `Gpu`,
/// `Cache`) take budgeted byte-accounting. `Permanent` holds data that lives for
/// the process.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Region {
    Permanent,
    Document,
    Parser,
    Frame,
    Images,
    Fonts,
    Gpu,
    Cache,
}

impl Region {
    /// Every region, in declaration order.
    ///
    /// The per-region accounting registry uses this to hold one counter set per
    /// region, so the order here fixes the registry layout.
    pub const ALL: [Region; 8] = [
        Region::Permanent,
        Region::Document,
        Region::Parser,
        Region::Frame,
        Region::Images,
        Region::Fonts,
        Region::Gpu,
        Region::Cache,
    ];

    /// The position of the region in [`Region::ALL`].
    ///
    /// This is the stable index into the per-region counter arrays.
    pub const fn index(self) -> usize {
        self as usize
    }
}

#[cfg(test)]
mod tests {
    use super::Region;

    #[test]
    fn all_lists_every_region_once() {
        assert_eq!(Region::ALL.len(), 8);
        for (position, region) in Region::ALL.iter().enumerate() {
            assert_eq!(region.index(), position);
        }
    }
}
