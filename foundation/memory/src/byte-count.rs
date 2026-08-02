// @file foundation/memory/src/byte-count.rs
// @description Defines the byte-count currency for budgets and accounting.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// A count of bytes.
///
/// This is the shared currency the budget and accounting types use, so byte
/// amounts are not confused with plain integers. It is a fixed-width `u64`
/// because a memory amount is not tied to an in-memory collection index.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ByteCount(u64);

impl ByteCount {
    /// A zero byte count.
    pub const ZERO: ByteCount = ByteCount(0);

    /// Wraps a raw byte amount.
    pub const fn new(bytes: u64) -> Self {
        Self(bytes)
    }

    /// The raw byte amount.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Adds two byte counts, or `None` on overflow.
    pub const fn checked_add(self, other: ByteCount) -> Option<ByteCount> {
        match self.0.checked_add(other.0) {
            Some(sum) => Some(ByteCount(sum)),
            None => None,
        }
    }

    /// Subtracts `other`, saturating at zero.
    ///
    /// Releasing more than is held must not underflow, so this saturates rather
    /// than wrapping.
    pub const fn saturating_sub(self, other: ByteCount) -> ByteCount {
        ByteCount(self.0.saturating_sub(other.0))
    }
}

#[cfg(test)]
mod tests {
    use super::ByteCount;

    #[test]
    fn checked_add_reports_overflow() {
        assert_eq!(
            ByteCount::new(2).checked_add(ByteCount::new(3)),
            Some(ByteCount::new(5))
        );
        assert_eq!(
            ByteCount::new(u64::MAX).checked_add(ByteCount::new(1)),
            None
        );
    }

    #[test]
    fn saturating_sub_stops_at_zero() {
        assert_eq!(
            ByteCount::new(5).saturating_sub(ByteCount::new(2)),
            ByteCount::new(3)
        );
        assert_eq!(
            ByteCount::new(2).saturating_sub(ByteCount::new(5)),
            ByteCount::ZERO
        );
    }
}
