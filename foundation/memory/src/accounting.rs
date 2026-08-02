// @file foundation/memory/src/accounting.rs
// @description Defines the per-region memory accounting registry.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::sync::atomic::{AtomicU64, Ordering};

use crate::byte_count::ByteCount;
use crate::region::Region;

/// The atomic counters of one region.
///
/// The counters use relaxed ordering because they are independent metrics, not a
/// lock protecting other state.
#[derive(Debug, Default)]
struct RegionCounters {
    resident: AtomicU64,
    peak: AtomicU64,
    allocation_count: AtomicU64,
    allocated_bytes: AtomicU64,
}

/// A read-only snapshot of one region's counters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegionSnapshot {
    pub resident: ByteCount,
    pub peak: ByteCount,
    pub allocation_count: u64,
    pub allocated_bytes: ByteCount,
}

/// The per-region memory accounting registry.
///
/// The registry holds one set of atomic counters per [`Region`] and no payload
/// state. It is a value the runtime owns and lends to subsystems; it is not a
/// process-wide `static`. Subsystems report allocations and releases into it,
/// and observers read it through a [`AccountingView`].
#[derive(Debug, Default)]
pub struct AccountingRegistry {
    regions: [RegionCounters; Region::ALL.len()],
}

impl AccountingRegistry {
    /// Creates a registry with every counter at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an allocation of `bytes` in `region`.
    ///
    /// Advances the allocation count, the cumulative allocated bytes, and the
    /// resident bytes, and raises the peak when the new resident amount exceeds
    /// it.
    pub fn record_allocation(&self, region: Region, bytes: ByteCount) {
        let counters = &self.regions[region.index()];
        counters.allocation_count.fetch_add(1, Ordering::Relaxed);
        counters
            .allocated_bytes
            .fetch_add(bytes.get(), Ordering::Relaxed);
        let resident = counters.resident.fetch_add(bytes.get(), Ordering::Relaxed) + bytes.get();
        counters.peak.fetch_max(resident, Ordering::Relaxed);
    }

    /// Records a release of `bytes` in `region`, saturating the resident amount
    /// at zero. The peak is not lowered.
    pub fn record_release(&self, region: Region, bytes: ByteCount) {
        let counters = &self.regions[region.index()];
        let _ = counters
            .resident
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |resident| {
                Some(resident.saturating_sub(bytes.get()))
            });
    }

    /// Borrows a read-only view of the registry.
    pub fn view(&self) -> AccountingView<'_> {
        AccountingView { registry: self }
    }
}

/// A read-only view of a [`AccountingRegistry`].
///
/// The view reports per-region counters and exposes no mutation.
#[derive(Clone, Copy, Debug)]
pub struct AccountingView<'a> {
    registry: &'a AccountingRegistry,
}

impl AccountingView<'_> {
    /// The current counters of `region`.
    pub fn region(&self, region: Region) -> RegionSnapshot {
        let counters = &self.registry.regions[region.index()];
        RegionSnapshot {
            resident: ByteCount::new(counters.resident.load(Ordering::Relaxed)),
            peak: ByteCount::new(counters.peak.load(Ordering::Relaxed)),
            allocation_count: counters.allocation_count.load(Ordering::Relaxed),
            allocated_bytes: ByteCount::new(counters.allocated_bytes.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AccountingRegistry;
    use crate::byte_count::ByteCount;
    use crate::region::Region;

    #[test]
    fn records_allocation_and_release() {
        let registry = AccountingRegistry::new();
        registry.record_allocation(Region::Images, ByteCount::new(100));
        registry.record_allocation(Region::Images, ByteCount::new(50));

        let snapshot = registry.view().region(Region::Images);
        assert_eq!(snapshot.resident, ByteCount::new(150));
        assert_eq!(snapshot.peak, ByteCount::new(150));
        assert_eq!(snapshot.allocation_count, 2);
        assert_eq!(snapshot.allocated_bytes, ByteCount::new(150));

        registry.record_release(Region::Images, ByteCount::new(120));
        let snapshot = registry.view().region(Region::Images);
        assert_eq!(snapshot.resident, ByteCount::new(30));
    }

    #[test]
    fn peak_never_decreases() {
        let registry = AccountingRegistry::new();
        registry.record_allocation(Region::Fonts, ByteCount::new(200));
        registry.record_release(Region::Fonts, ByteCount::new(200));
        registry.record_allocation(Region::Fonts, ByteCount::new(10));

        let snapshot = registry.view().region(Region::Fonts);
        assert_eq!(snapshot.resident, ByteCount::new(10));
        assert_eq!(snapshot.peak, ByteCount::new(200));
    }

    #[test]
    fn release_below_zero_saturates_resident() {
        let registry = AccountingRegistry::new();
        registry.record_allocation(Region::Cache, ByteCount::new(10));
        registry.record_release(Region::Cache, ByteCount::new(40));
        assert_eq!(
            registry.view().region(Region::Cache).resident,
            ByteCount::ZERO
        );
    }

    #[test]
    fn regions_are_independent() {
        let registry = AccountingRegistry::new();
        registry.record_allocation(Region::Gpu, ByteCount::new(64));

        assert_eq!(
            registry.view().region(Region::Gpu).resident,
            ByteCount::new(64)
        );
        assert_eq!(
            registry.view().region(Region::Document).resident,
            ByteCount::ZERO
        );
    }
}
