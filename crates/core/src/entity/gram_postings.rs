//! CSR bigram postings with block-packed increasing token ids.

use std::collections::BTreeMap;
use std::mem::size_of;

use super::TokenId;
use crate::{Error, Result};

const BLOCK_IDS: usize = 128;

#[derive(Debug, Clone, Default)]
pub(super) struct GramPostings {
  keys: Vec<(char, char)>,
  offsets: Vec<u32>,
  data: Vec<u8>,
}

fn invalid_postings() -> Error {
  Error::from_reason("Invalid entity bigram postings")
}

impl GramPostings {
  pub(super) fn new(
    lists: BTreeMap<(char, char), Vec<TokenId>>,
  ) -> Result<Self> {
    let mut store = Self {
      keys: Vec::with_capacity(lists.len()),
      offsets: vec![0],
      data: Vec::new(),
    };
    for (key, ids) in lists {
      store.keys.push(key);
      for block in ids.chunks(BLOCK_IDS) {
        let Some(&base) = block.first() else { continue };
        let mut maximum_gap = 0_u32;
        for (&left, &right) in block.iter().zip(block.iter().skip(1)) {
          let gap = right
            .checked_sub(left)
            .filter(|gap| *gap > 0)
            .ok_or_else(invalid_postings)?;
          maximum_gap = maximum_gap.max(gap);
        }
        let width =
          u8::try_from(u32::BITS.saturating_sub(maximum_gap.leading_zeros()))
            .map_err(|_| invalid_postings())?;
        store.data.push(
          u8::try_from(block.len().saturating_sub(1))
            .map_err(|_| invalid_postings())?,
        );
        store.data.push(width);
        store.data.extend_from_slice(&base.to_le_bytes());
        let mut buffer = 0_u64;
        let mut available = 0_u8;
        for (&left, &right) in block.iter().zip(block.iter().skip(1)) {
          let gap = right.checked_sub(left).ok_or_else(invalid_postings)?;
          buffer |= u64::from(gap) << available;
          available = available.saturating_add(width);
          while available >= 8 {
            store.data.push(buffer.to_le_bytes()[0]);
            buffer >>= 8;
            available = available.saturating_sub(8);
          }
        }
        if available != 0 {
          store.data.push(buffer.to_le_bytes()[0]);
        }
      }
      store
        .offsets
        .push(u32::try_from(store.data.len()).map_err(|_| {
          Error::from_reason("Entity bigram postings exceed u32")
        })?);
    }
    store.offsets.shrink_to_fit();
    store.data.shrink_to_fit();
    Ok(store)
  }

  /// Stream directly into the query's counters; no decoded list allocation.
  pub(super) fn for_each(
    &self,
    key: (char, char),
    mut visit: impl FnMut(TokenId) -> Result<()>,
  ) -> Result<()> {
    let Ok(index) = self.keys.binary_search(&key) else {
      return Ok(());
    };
    let start =
      usize::try_from(*self.offsets.get(index).ok_or_else(invalid_postings)?)
        .map_err(|_| invalid_postings())?;
    let end = usize::try_from(
      *self
        .offsets
        .get(index.saturating_add(1))
        .ok_or_else(invalid_postings)?,
    )
    .map_err(|_| invalid_postings())?;
    let mut bytes = self.data.get(start..end).ok_or_else(invalid_postings)?;
    while !bytes.is_empty() {
      let [gaps, width, a, b, c, d]: [u8; 6] = bytes
        .get(..6)
        .ok_or_else(invalid_postings)?
        .try_into()
        .map_err(|_| invalid_postings())?;
      let gaps = usize::from(gaps);
      if width > 32 {
        return Err(invalid_postings());
      }
      let mut id = u32::from_le_bytes([a, b, c, d]);
      visit(id)?;
      bytes = bytes.get(6..).ok_or_else(invalid_postings)?;
      let byte_count = gaps.saturating_mul(usize::from(width)).div_ceil(8);
      let mut payload =
        bytes.get(..byte_count).ok_or_else(invalid_postings)?.iter();
      let mask = (1_u64 << width).saturating_sub(1);
      let mut buffer = 0_u64;
      let mut available = 0_u8;
      for _ in 0..gaps {
        while available < width {
          buffer |= u64::from(*payload.next().ok_or_else(invalid_postings)?)
            << available;
          available = available.saturating_add(8);
        }
        let gap =
          u32::try_from(buffer & mask).map_err(|_| invalid_postings())?;
        buffer >>= width;
        available = available.saturating_sub(width);
        id = id.checked_add(gap).ok_or_else(invalid_postings)?;
        visit(id)?;
      }
      bytes = bytes.get(byte_count..).ok_or_else(invalid_postings)?;
    }
    Ok(())
  }

  pub(super) const fn heap_bytes(&self) -> usize {
    self
      .keys
      .capacity()
      .saturating_mul(size_of::<(char, char)>())
      .saturating_add(self.offsets.capacity().saturating_mul(size_of::<u32>()))
      .saturating_add(self.data.capacity())
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;
  use proptest::prelude::*;

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    #[test]
    fn csr_preserves_unicode_keys_and_all_increasing_u32_ids(
      rows in prop::collection::btree_map((any::<char>(), any::<char>()), prop::collection::btree_set(any::<u32>(), 0..300), 0..8),
    ) {
      let lists = rows.into_iter().map(|(key, ids)| (key, ids.into_iter().collect::<Vec<_>>())).collect::<BTreeMap<_, _>>();
      let store = GramPostings::new(lists.clone()).unwrap();
      for (key, expected) in lists {
        let mut actual = Vec::new();
        store.for_each(key, |id| { actual.push(id); Ok(()) }).unwrap();
        prop_assert_eq!(actual, expected);
      }
    }
  }

  #[test]
  fn block_boundaries_zero_and_full_width_gaps() {
    for count in [0_u32, 1, 127, 128, 129, 256, 257] {
      let mut ids = (0..count).collect::<Vec<_>>();
      ids.push(u32::MAX);
      let store =
        GramPostings::new(BTreeMap::from([(('^', '\u{10ffff}'), ids.clone())]))
          .unwrap();
      let mut actual = Vec::new();
      store
        .for_each(('^', '\u{10ffff}'), |id| {
          actual.push(id);
          Ok(())
        })
        .unwrap();
      assert_eq!(actual, ids);
      let mut absent_visits = 0;
      store
        .for_each(('a', 'b'), |_| {
          absent_visits += 1;
          Ok(())
        })
        .unwrap();
      assert_eq!(absent_visits, 0);
    }
  }
}
