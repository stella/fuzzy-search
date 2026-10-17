//! Compact impact-ordered inverted posting lists.

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::size_of;

use crate::{Error, Result};

use super::AliasId;
use super::search::PostingList;

#[derive(Debug, Default)]
pub(super) struct PostingBuilder {
  lists: BTreeMap<PostingList, BTreeMap<AliasId, f64>>,
}

#[derive(Debug, Default)]
struct ListTable {
  keys: Vec<u64>,
  ranges: Vec<u32>,
}

#[derive(Debug, Default)]
pub(super) struct Postings {
  tables: [ListTable; 4],
  ranges: Vec<ListRange>,
  quanta: Vec<f64>,
  data: Vec<u8>,
  len: u64,
}

// Singleton lists need no payload. Constant impacts are stored once, while
// alias ids use only the bits needed for their range relative to `base`.
#[derive(Debug, Clone, Copy)]
struct ListRange {
  start: u32,
  len: u32,
  quantum: u32,
  base: u32,
  impact: u16,
  alias_bits: u8,
  impact_bits: u8,
}

fn list_key(list: PostingList) -> (usize, u64) {
  let (kind, form, token) = match list {
    PostingList::Token { form, id } => (0, form, id),
    PostingList::Join { form, id } => (1, form, id),
    PostingList::QueryInitial { form, prefix } => (2, form, u32::from(prefix)),
    PostingList::ListedInitial { form, prefix } => (3, form, u32::from(prefix)),
  };
  (kind, (u64::from(form) << 32) | u64::from(token))
}

fn compact_u32(value: usize) -> Result<u32> {
  u32::try_from(value)
    .map_err(|_| Error::from_reason("Compact posting storage exceeds u32"))
}

impl PostingBuilder {
  pub(super) fn add(
    &mut self,
    list: PostingList,
    alias: AliasId,
    impact: f64,
  ) -> Result<()> {
    if !impact.is_finite() || impact < 0.0 {
      return Err(Error::from_reason(
        "Posting impacts must be finite and nonnegative",
      ));
    }
    let total = self
      .lists
      .entry(list)
      .or_default()
      .entry(alias)
      .or_default();
    let sum = *total + impact;
    if !sum.is_finite() {
      return Err(Error::from_reason("Posting impact sum exceeds f64"));
    }
    let upper = if *total == 0.0 || impact == 0.0 {
      sum
    } else {
      sum.next_up()
    };
    if !upper.is_finite() {
      return Err(Error::from_reason(
        "Posting impact sum exceeds finite upward range",
      ));
    }
    *total = upper;
    Ok(())
  }

  pub(super) fn finish(self) -> Result<Postings> {
    let mut postings = Postings::default();
    let mut quantum_ids = BTreeMap::new();
    let mut payload_ids: HashMap<u64, Vec<u32>> = HashMap::new();
    for (list, values) in self.lists {
      let maximum = values.values().copied().fold(0.0_f64, f64::max);
      let quantum = if maximum == 0.0 {
        0.0
      } else {
        next_up(maximum / f64::from(u16::MAX))
      };
      let mut entries = values
        .into_iter()
        .map(|(alias, impact)| Ok((alias, encode_impact(impact, quantum)?)))
        .collect::<Result<Vec<_>>>()?;
      entries.sort_unstable_by(
        |(left_alias, left_impact), (right_alias, right_impact)| {
          decode_impact(*right_impact, quantum)
            .total_cmp(&decode_impact(*left_impact, quantum))
            .then_with(|| left_alias.cmp(right_alias))
        },
      );
      postings.len = postings
        .len
        .checked_add(
          u64::try_from(entries.len())
            .map_err(|_| Error::from_reason("Posting list exceeds u64"))?,
        )
        .ok_or_else(|| Error::from_reason("Posting count exceeds u64"))?;
      let quantum_id = if let Some(id) = quantum_ids.get(&quantum.to_bits()) {
        *id
      } else {
        let id = compact_u32(postings.quanta.len())?;
        postings.quanta.push(quantum);
        quantum_ids.insert(quantum.to_bits(), id);
        id
      };
      let mut hash = DefaultHasher::new();
      quantum_id.hash(&mut hash);
      entries.hash(&mut hash);
      let hash = hash.finish();
      // Hashes only select candidates; equality checks every encoded entry.
      let mut reused = None;
      for id in payload_ids.get(&hash).into_iter().flatten().copied() {
        let range = postings
          .ranges
          .get(usize::try_from(id).map_err(|_| {
            Error::from_reason("Posting descriptor index exceeds usize")
          })?)
          .ok_or_else(|| {
            Error::from_reason("Missing interned posting descriptor")
          })?;
        let len = usize::try_from(range.len).map_err(|_| {
          Error::from_reason("Posting descriptor length exceeds usize")
        })?;
        if range.quantum != quantum_id || len != entries.len() {
          continue;
        }
        let mut equal = true;
        for (offset, entry) in entries.iter().enumerate() {
          let stored = postings.encoded(*range, offset).ok_or_else(|| {
            Error::from_reason("Invalid interned posting payload")
          })?;
          if stored != *entry {
            equal = false;
            break;
          }
        }
        if equal {
          reused = Some(id);
          break;
        }
      }
      let range_id = if let Some(id) = reused {
        id
      } else {
        let id = compact_u32(postings.ranges.len())?;
        postings.store(&entries, quantum_id)?;
        payload_ids.entry(hash).or_default().push(id);
        id
      };
      let (kind, key) = list_key(list);
      let table = postings
        .tables
        .get_mut(kind)
        .ok_or_else(|| Error::from_reason("Unknown posting kind"))?;
      table.keys.push(key);
      table.ranges.push(range_id);
    }
    for table in &mut postings.tables {
      table.keys.shrink_to_fit();
      table.ranges.shrink_to_fit();
    }
    postings.ranges.shrink_to_fit();
    postings.quanta.shrink_to_fit();
    postings.data.shrink_to_fit();
    Ok(postings)
  }
}

impl Postings {
  fn store(&mut self, entries: &[(AliasId, u16)], quantum: u32) -> Result<()> {
    let base = entries
      .iter()
      .map(|(alias, _)| *alias)
      .min()
      .ok_or_else(|| Error::from_reason("Empty posting list"))?;
    let maximum = entries
      .iter()
      .map(|(alias, _)| *alias)
      .max()
      .ok_or_else(|| Error::from_reason("Empty posting list"))?;
    // `base` is the minimum alias and leading_zeros is at most u32::BITS.
    let alias_bits = u8::try_from(
      u32::BITS.saturating_sub(maximum.saturating_sub(base).leading_zeros()),
    )
    .map_err(|_| Error::from_reason("Invalid posting bit width"))?;
    let impact = entries
      .first()
      .ok_or_else(|| Error::from_reason("Empty posting list"))?
      .1;
    let impact_bits = if entries.iter().all(|(_, value)| *value == impact) {
      0
    } else {
      16
    };
    let bits = usize::from(alias_bits).saturating_add(usize::from(impact_bits));
    let bytes = entries
      .len()
      .checked_mul(bits)
      .and_then(|value| value.checked_add(7))
      .map(|value| value >> 3)
      .ok_or_else(|| Error::from_reason("Posting bit count overflow"))?;
    let start = self.data.len();
    let end = start
      .checked_add(bytes)
      .ok_or_else(|| Error::from_reason("Posting byte count overflow"))?;
    compact_u32(end)?;
    self.data.resize(end, 0);
    for (offset, (alias, value)) in entries.iter().enumerate() {
      let packed = u64::from(alias.saturating_sub(base))
        | if impact_bits == 0 {
          0
        } else {
          u64::from(*value) << alias_bits
        };
      let bit = offset
        .checked_mul(bits)
        .ok_or_else(|| Error::from_reason("Posting bit offset overflow"))?;
      let encoded = (packed << (bit % 8)).to_le_bytes();
      let width = bits.saturating_add(bit % 8).div_ceil(8);
      let position = start
        .checked_add(bit >> 3)
        .ok_or_else(|| Error::from_reason("Posting byte offset overflow"))?;
      let record_end = position
        .checked_add(width)
        .ok_or_else(|| Error::from_reason("Posting byte offset overflow"))?;
      for (target, byte) in self
        .data
        .get_mut(position..record_end)
        .ok_or_else(|| {
          Error::from_reason("Posting payload outside allocated range")
        })?
        .iter_mut()
        .zip(encoded.iter())
        .take(width)
      {
        *target |= byte;
      }
    }
    self.ranges.push(ListRange {
      start: compact_u32(start)?,
      len: compact_u32(entries.len())?,
      quantum,
      base,
      impact,
      alias_bits,
      impact_bits,
    });
    Ok(())
  }

  fn encoded(&self, range: ListRange, offset: usize) -> Option<(AliasId, u16)> {
    if offset >= usize::try_from(range.len).ok()? {
      return None;
    }
    // Constant singleton records live entirely in the descriptor.
    if range.alias_bits == 0 && range.impact_bits == 0 {
      return Some((range.base, range.impact));
    }
    let bits = usize::from(range.alias_bits)
      .saturating_add(usize::from(range.impact_bits));
    let bit = offset.checked_mul(bits)?;
    let start = usize::try_from(range.start).ok()?.checked_add(bit >> 3)?;
    let width = bits.saturating_add(bit % 8).div_ceil(8);
    let mut bytes = [0; 8];
    bytes
      .get_mut(..width)?
      .copy_from_slice(self.data.get(start..start.checked_add(width)?)?);
    let packed = u64::from_le_bytes(bytes) >> (bit % 8);
    let relative =
      u32::try_from(packed & (1_u64 << range.alias_bits).saturating_sub(1))
        .ok()?;
    let impact = if range.impact_bits == 0 {
      range.impact
    } else {
      u16::try_from((packed >> range.alias_bits) & u64::from(u16::MAX)).ok()?
    };
    Some((range.base.checked_add(relative)?, impact))
  }

  pub(super) fn get(
    &self,
    list: PostingList,
    offset: usize,
  ) -> Option<(AliasId, f64)> {
    let (kind, key) = list_key(list);
    let table = self.tables.get(kind)?;
    let index = table.keys.binary_search(&key).ok()?;
    let range = *self
      .ranges
      .get(usize::try_from(*table.ranges.get(index)?).ok()?)?;
    let (alias, impact) = self.encoded(range, offset)?;
    Some((
      alias,
      decode_impact(
        impact,
        *self.quanta.get(usize::try_from(range.quantum).ok()?)?,
      ),
    ))
  }

  #[cfg(test)]
  #[allow(clippy::expect_used)]
  pub(super) fn list_keys(&self) -> Vec<PostingList> {
    let [tokens, joins, query_initials, listed_initials] = &self.tables;
    let parts = |key: &u64| {
      (
        u32::try_from(key >> 32).expect("form key fits u32"),
        u32::try_from(key & u64::from(u32::MAX)).expect("token key fits u32"),
      )
    };
    tokens
      .keys
      .iter()
      .map(parts)
      .map(|(form, id)| PostingList::Token { form, id })
      .chain(
        joins
          .keys
          .iter()
          .map(parts)
          .map(|(form, id)| PostingList::Join { form, id }),
      )
      .chain(query_initials.keys.iter().map(parts).map(|(form, prefix)| {
        PostingList::QueryInitial {
          form,
          prefix: char::from_u32(prefix)
            .expect("query-initial key is a Unicode scalar"),
        }
      }))
      .chain(
        listed_initials
          .keys
          .iter()
          .map(parts)
          .map(|(form, prefix)| PostingList::ListedInitial {
            form,
            prefix: u16::try_from(prefix)
              .expect("listed-initial key fits UTF-16"),
          }),
      )
      .collect()
  }

  pub(super) const fn len(&self) -> u64 {
    self.len
  }

  pub(super) fn native_bytes(&self) -> usize {
    self
      .tables
      .iter()
      .fold(size_of::<Self>(), |bytes, table| {
        bytes
          .saturating_add(
            table.keys.capacity().saturating_mul(size_of::<u64>()),
          )
          .saturating_add(
            table.ranges.capacity().saturating_mul(size_of::<u32>()),
          )
      })
      .saturating_add(
        self
          .ranges
          .capacity()
          .saturating_mul(size_of::<ListRange>()),
      )
      .saturating_add(self.quanta.capacity().saturating_mul(size_of::<f64>()))
      .saturating_add(self.data.capacity())
  }
}

fn next_up(value: f64) -> f64 {
  debug_assert!(
    value.is_finite() && value >= 0.0,
    "posting quantum must be finite and nonnegative"
  );
  if value == 0.0 {
    return f64::from_bits(1);
  }
  f64::from_bits(value.to_bits().saturating_add(1))
}

fn encode_impact(impact: f64, quantum: f64) -> Result<u16> {
  if impact == 0.0 {
    return Ok(0);
  }
  let scaled = (impact / quantum).ceil();
  if !scaled.is_finite() || scaled > f64::from(u16::MAX) {
    return Err(Error::from_reason("Posting impact cannot be quantized"));
  }
  // `scaled` is finite, nonnegative, integral, and bounded by u16::MAX.
  #[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions
  )]
  let mut encoded = scaled as u16;
  if decode_impact(encoded, quantum) < impact {
    encoded = encoded.checked_add(1).ok_or_else(|| {
      Error::from_reason("Posting impact quantization overflow")
    })?;
  }
  if !decode_impact(encoded, quantum).is_finite() {
    return Err(Error::from_reason(
      "Posting impact exceeds finite quantized range",
    ));
  }
  Ok(encoded)
}

fn decode_impact(impact: u16, quantum: f64) -> f64 {
  f64::from(impact) * quantum
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
  use super::*;
  use proptest::prelude::*;

  fn list() -> PostingList {
    PostingList::Token { form: 0, id: 0 }
  }

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn quantized_impacts_never_fall_below_raw(
      raw in prop::collection::vec(0.0_f64..1_000_000.0, 1..64),
    ) {
      let mut builder = PostingBuilder::default();
      for (alias, impact) in raw.iter().copied().enumerate() {
        builder.add(list(), u32::try_from(alias).unwrap(), impact).unwrap();
      }
      let postings = builder.finish().unwrap();
      for offset in 0..raw.len() {
        let (alias, decoded) = postings.get(list(), offset).unwrap();
        prop_assert!(decoded >= *raw.get(usize::try_from(alias).unwrap()).unwrap());
      }
    }

    #[test]
    fn packed_entries_roundtrip_all_alias_and_impact_bits(
      entries in prop::collection::vec((any::<u32>(), any::<u16>()), 1..128),
    ) {
      let mut postings = Postings::default();
      postings.store(&entries, 0).unwrap();
      let range = *postings.ranges.first().unwrap();
      for (offset, entry) in entries.iter().enumerate() {
        prop_assert_eq!(postings.encoded(range, offset), Some(*entry));
      }
      prop_assert_eq!(postings.encoded(range, entries.len()), None);
      prop_assert_eq!(postings.encoded(range, usize::MAX), None);
    }

    #[test]
    fn arbitrary_alias_ids_preserve_upward_impacts_and_total_order(
      values in prop::collection::btree_map(any::<u32>(), 0.0_f64..1_000_000.0, 1..128),
    ) {
      let mut builder = PostingBuilder::default();
      for (alias, impact) in &values {
        builder.add(list(), *alias, *impact).unwrap();
      }
      let postings = builder.finish().unwrap();
      let mut previous = None;
      for offset in 0..values.len() {
        let (alias, impact) = postings.get(list(), offset).unwrap();
        prop_assert!(impact >= *values.get(&alias).unwrap());
        if let Some((previous_alias, previous_impact)) = previous {
          prop_assert!(impact < previous_impact || (impact.total_cmp(&previous_impact).is_eq() && alias > previous_alias));
        }
        previous = Some((alias, impact));
      }
      prop_assert_eq!(postings.len(), u64::try_from(values.len()).unwrap());
      prop_assert_eq!(postings.get(list(), values.len()), None);
    }
  }

  #[test]
  fn identical_payloads_share_storage_across_forms_and_kinds() {
    let lists = [
      list(),
      PostingList::Join {
        form: 1,
        id: u32::MAX,
      },
      PostingList::QueryInitial {
        form: u32::MAX,
        prefix: '𐐀',
      },
      PostingList::ListedInitial {
        form: 1,
        prefix: u16::MAX,
      },
    ];
    let mut builder = PostingBuilder::default();
    for key in lists {
      builder.add(key, u32::MAX, 2.0).unwrap();
    }
    let postings = builder.finish().unwrap();
    assert_eq!(postings.ranges.len(), 1);
    assert_eq!(postings.quanta.len(), 1);
    assert!(postings.data.is_empty());
    assert_eq!(postings.len(), 4);
    for key in lists {
      let (alias, impact) = postings.get(key, 0).unwrap();
      assert_eq!(alias, u32::MAX);
      assert!(impact >= 2.0);
      assert_eq!(postings.get(key, 1), None);
    }
  }

  #[test]
  fn maximum_width_packed_records_cross_byte_boundaries() {
    let entries = [
      (0, 0),
      (u32::MAX, u16::MAX),
      (123, 456),
      (u32::MAX - 1, 789),
    ];
    let mut postings = Postings::default();
    postings.store(&entries, 0).unwrap();
    let range = *postings.ranges.first().unwrap();
    assert_eq!(range.alias_bits, 32);
    assert_eq!(range.impact_bits, 16);
    for (offset, entry) in entries.iter().enumerate() {
      assert_eq!(postings.encoded(range, offset), Some(*entry));
    }
    // Five alias bits plus sixteen impact bits forces non-byte-aligned reads.
    let narrow_entries = [(3, 0), (31, u16::MAX), (4, 1), (5, 2), (6, 3)];
    postings.store(&narrow_entries, 0).unwrap();
    let narrow_range = *postings.ranges.last().unwrap();
    assert_eq!(narrow_range.alias_bits, 5);
    for (offset, entry) in narrow_entries.iter().enumerate() {
      assert_eq!(postings.encoded(narrow_range, offset), Some(*entry));
    }
  }

  #[test]
  fn combines_duplicate_aliases_and_orders_decoded_impacts() {
    let mut builder = PostingBuilder::default();
    builder.add(list(), 9, 1.25).unwrap();
    builder.add(list(), 3, 2.0).unwrap();
    builder.add(list(), 9, 2.75).unwrap();
    builder.add(list(), 5, 4.0).unwrap();
    let postings = builder.finish().unwrap();

    assert_eq!(postings.len(), 3);
    assert_eq!(postings.get(list(), 0).map(|(alias, _)| alias), Some(5));
    assert!(postings.get(list(), 0).unwrap().1 >= 4.0);
    assert_eq!(postings.get(list(), 1).map(|(alias, _)| alias), Some(9));
    assert_eq!(postings.get(list(), 2).map(|(alias, _)| alias), Some(3));
    assert_eq!(postings.get(list(), 3), None);
    assert_eq!(postings.get(list(), usize::MAX), None);
  }

  #[test]
  fn rejects_invalid_impacts_and_overflowed_sums() {
    let mut builder = PostingBuilder::default();
    assert!(builder.add(list(), 0, f64::NAN).is_err());
    assert!(builder.add(list(), 0, -1.0).is_err());
    builder.add(list(), 0, f64::MAX).unwrap();
    assert!(builder.add(list(), 0, f64::MAX).is_err());
    assert!(builder.finish().is_err());

    // The sum is finite, but its upward bound would exceed the finite range.
    let half_maximum = f64::MAX / 2.0;
    let mut upward = PostingBuilder::default();
    upward.add(list(), 0, half_maximum).unwrap();
    assert!(upward.add(list(), 0, half_maximum).is_err());
    let retained = upward.finish().unwrap();
    let (_, impact) = retained.get(list(), 0).unwrap();
    assert!(impact.is_finite() && impact >= half_maximum);
  }
}
