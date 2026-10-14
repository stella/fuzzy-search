//! Deterministic work accounting shared by lookup and threshold search.

use super::Cost;

#[derive(Debug, Clone, Copy)]
pub(super) enum Work {
  SortedAccess,
  RandomAccess(u64),
  Survivor,
}

impl Cost {
  /// Total charged work. Saturation keeps diagnostics monotone at overflow.
  #[must_use]
  pub fn total(self) -> u64 {
    self
      .lexicon_cells
      .saturating_add(self.lexicon_matches)
      .saturating_add(self.sorted_accesses)
      .saturating_add(self.random_access_pairs)
      .saturating_add(self.survivors)
  }

  /// Charge an indivisible operation before doing it. Failure changes nothing.
  pub(super) fn charge(&mut self, work: Work, maximum: u64) -> bool {
    let amount = match work {
      Work::SortedAccess | Work::Survivor => 1,
      Work::RandomAccess(pairs) => pairs,
    };
    let Some(next) = self.total().checked_add(amount) else {
      return false;
    };
    if next > maximum {
      return false;
    }
    match work {
      Work::SortedAccess => self.sorted_accesses += amount,
      Work::RandomAccess(_) => self.random_access_pairs += amount,
      Work::Survivor => self.survivors += amount,
    }
    true
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn failed_charge_is_atomic_and_overflow_is_exhaustion() {
    let mut cost = Cost::default();
    assert!(cost.charge(Work::RandomAccess(7), 8));
    assert!(!cost.charge(Work::RandomAccess(2), 8));
    assert_eq!(cost.total(), 7);
    assert!(cost.charge(Work::SortedAccess, 8));
    assert!(!cost.charge(Work::Survivor, 8));
    cost.random_access_pairs = u64::MAX;
    assert!(!cost.charge(Work::SortedAccess, u64::MAX));
  }
}
