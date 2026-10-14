//! Compact impact-ordered inverted posting lists.

use std::collections::BTreeMap;
use std::mem::size_of;

use crate::{Error, Result};

use super::AliasId;
use super::search::PostingList;

#[derive(Debug, Default)]
pub(super) struct PostingBuilder {
  lists: BTreeMap<PostingList, BTreeMap<AliasId, f64>>,
}

#[derive(Debug, Default)]
pub(super) struct Postings {
  lists: Vec<(PostingList, ListRange)>,
  aliases: Vec<AliasId>,
  impacts: Vec<u16>,
  len: u64,
}

#[derive(Debug, Clone, Copy)]
struct ListRange {
  start: usize,
  end: usize,
  quantum: f64,
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
    let total = self
      .lists
      .values()
      .try_fold(0_usize, |count, entries| count.checked_add(entries.len()))
      .ok_or_else(|| Error::from_reason("Posting count exceeds usize"))?;
    let mut postings = Postings::default();
    postings.lists.reserve(self.lists.len());
    postings.aliases.reserve(total);
    postings.impacts.reserve(total);
    for (list, values) in self.lists {
      let maximum = values.values().copied().fold(0.0_f64, f64::max);
      let quantum = if maximum == 0.0 {
        0.0
      } else {
        next_up(maximum / f64::from(u16::MAX))
      };
      let mut entries = values
        .into_iter()
        .map(|(alias, impact)| {
          let encoded = encode_impact(impact, quantum)?;
          Ok((alias, encoded))
        })
        .collect::<Result<Vec<_>>>()?;
      entries.sort_unstable_by(
        |(left_alias, left_impact), (right_alias, right_impact)| {
          decode_impact(*right_impact, quantum)
            .total_cmp(&decode_impact(*left_impact, quantum))
            .then_with(|| left_alias.cmp(right_alias))
        },
      );

      let list_len = u64::try_from(entries.len())
        .map_err(|_| Error::from_reason("Posting list exceeds u64 length"))?;
      postings.len = postings
        .len
        .checked_add(list_len)
        .ok_or_else(|| Error::from_reason("Posting count exceeds u64"))?;
      let start = postings.aliases.len();
      for (alias, impact) in entries {
        postings.aliases.push(alias);
        postings.impacts.push(impact);
      }
      postings.lists.push((
        list,
        ListRange {
          start,
          end: postings.aliases.len(),
          quantum,
        },
      ));
    }
    Ok(postings)
  }
}

impl Postings {
  pub(super) fn get(
    &self,
    list: PostingList,
    offset: usize,
  ) -> Option<(AliasId, f64)> {
    let list_index = self
      .lists
      .binary_search_by_key(&list, |(key, _)| *key)
      .ok()?;
    let range = self.lists.get(list_index)?.1;
    let index = range.start.checked_add(offset)?;
    if index >= range.end {
      return None;
    }
    let alias = *self.aliases.get(index)?;
    let impact = *self.impacts.get(index)?;
    Some((alias, decode_impact(impact, range.quantum)))
  }

  pub(super) const fn len(&self) -> u64 {
    self.len
  }

  pub(super) const fn native_bytes(&self) -> usize {
    size_of::<Self>()
      .saturating_add(
        self
          .lists
          .capacity()
          .saturating_mul(size_of::<(PostingList, ListRange)>()),
      )
      .saturating_add(
        self.aliases.capacity().saturating_mul(size_of::<AliasId>()),
      )
      .saturating_add(self.impacts.capacity().saturating_mul(size_of::<u16>()))
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
  }
}
