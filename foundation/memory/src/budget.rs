// @file foundation/memory/src/budget.rs
// @description Defines the memory budget and its reservation error.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::byte_count::ByteCount;

/// The error a budget returns when a reservation would exceed its maximum.
///
/// The reported values are byte counts only. They carry no path, identifier, or
/// other sensitive value.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum BudgetError {
    #[error("memory reservation of {requested} bytes exceeds the available {available} bytes")]
    Exceeded { requested: u64, available: u64 },
}

/// A byte budget for one capacity region or one cache.
///
/// A budget enforces a maximum. It fails closed: a reservation that would exceed
/// the maximum is rejected and leaves the used amount unchanged. The crate ships
/// the mechanism; the numeric maximum is Panther policy set by the owner.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    max: ByteCount,
    used: ByteCount,
}

impl Budget {
    /// Creates a budget with the given maximum and no used bytes.
    pub const fn new(max: ByteCount) -> Self {
        Self {
            max,
            used: ByteCount::ZERO,
        }
    }

    /// The maximum this budget allows.
    pub const fn max(&self) -> ByteCount {
        self.max
    }

    /// The bytes currently reserved.
    pub const fn used(&self) -> ByteCount {
        self.used
    }

    /// The bytes still available.
    pub const fn remaining(&self) -> ByteCount {
        self.max.saturating_sub(self.used)
    }

    /// Reserves `amount` bytes, or fails without changing the used amount.
    ///
    /// The reservation fails when the new total would exceed the maximum or
    /// overflow. Failure is a rejection, not a partial reservation.
    pub fn try_reserve(&mut self, amount: ByteCount) -> Result<(), BudgetError> {
        let exceeded = || BudgetError::Exceeded {
            requested: amount.get(),
            available: self.remaining().get(),
        };

        let next = self.used.checked_add(amount).ok_or_else(exceeded)?;
        if next > self.max {
            return Err(exceeded());
        }

        self.used = next;
        Ok(())
    }

    /// Releases `amount` bytes, saturating at zero.
    pub fn release(&mut self, amount: ByteCount) {
        self.used = self.used.saturating_sub(amount);
    }
}

#[cfg(test)]
mod tests {
    use super::{Budget, BudgetError};
    use crate::byte_count::ByteCount;

    #[test]
    fn reserve_within_the_limit_succeeds() {
        let mut budget = Budget::new(ByteCount::new(100));
        assert!(budget.try_reserve(ByteCount::new(60)).is_ok());
        assert_eq!(budget.used(), ByteCount::new(60));
        assert_eq!(budget.remaining(), ByteCount::new(40));
    }

    #[test]
    fn reserve_over_the_limit_returns_an_error_and_keeps_the_used_amount() {
        let mut budget = Budget::new(ByteCount::new(100));
        budget.try_reserve(ByteCount::new(80)).unwrap();

        let result = budget.try_reserve(ByteCount::new(40));
        assert_eq!(
            result,
            Err(BudgetError::Exceeded {
                requested: 40,
                available: 20
            })
        );
        assert_eq!(budget.used(), ByteCount::new(80));
    }

    #[test]
    fn release_restores_capacity() {
        let mut budget = Budget::new(ByteCount::new(100));
        budget.try_reserve(ByteCount::new(100)).unwrap();
        assert!(budget.try_reserve(ByteCount::new(1)).is_err());

        budget.release(ByteCount::new(30));
        assert_eq!(budget.remaining(), ByteCount::new(30));
        assert!(budget.try_reserve(ByteCount::new(30)).is_ok());
    }

    #[test]
    fn release_below_zero_saturates() {
        let mut budget = Budget::new(ByteCount::new(100));
        budget.try_reserve(ByteCount::new(10)).unwrap();
        budget.release(ByteCount::new(50));
        assert_eq!(budget.used(), ByteCount::ZERO);
    }
}
