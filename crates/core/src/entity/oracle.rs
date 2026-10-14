//! Independent test port of the legacy TypeScript alias scorer and estimate.
//!
//! Keep the greedy alignment and coverage arithmetic explicit here so changes to
//! retrieval do not silently change the reference semantics.

use std::collections::BTreeMap;

// Oracle inputs are validated production ids; conversion failure is a test failure.
#[allow(clippy::expect_used)]
fn position(value: u32) -> usize {
  usize::try_from(value).expect("validated entity position fits usize")
}

#[derive(Clone, Debug)]
pub(super) struct Pair {
  pub word: bool,
  pub query: Vec<usize>,
  pub listed: Vec<usize>,
  pub similarity: f64,
}

/// Build candidates independently from the retrieval implementation. Forms sharing
/// positions represent alternate spellings; their similarities combine by maximum.
// Alias forms supplied to the oracle have the same validated token count.
#[allow(clippy::expect_used)]
pub(super) fn candidate_pairs(
  query: &super::EntityQuery,
  alias: &super::AliasInput,
  matches: &[super::UnitMatch],
  mut token_text: impl FnMut(super::FormId, super::TokenId) -> String,
  initial_similarity: f64,
) -> Vec<Pair> {
  let mut pairs: Vec<Pair> = Vec::new();
  let mut insert = |pair: Pair| {
    if let Some(previous) = pairs.iter_mut().find(|previous| {
      previous.word == pair.word
        && previous.query == pair.query
        && previous.listed == pair.listed
    }) {
      previous.similarity = previous.similarity.max(pair.similarity);
    } else {
      pairs.push(pair);
    }
  };
  // Group alternate forms before visiting listed positions, matching legacy
  // max(raw, folded) evaluation rather than aligning each spelling separately.
  let mut positions = Vec::new();
  for unit in &query.units {
    if !positions.contains(&unit.positions) {
      positions.push(unit.positions.clone());
    }
  }
  for query_positions in positions {
    let listed_len = alias.forms.first().map_or(0, |form| form.tokens.len());
    for listed in 0..listed_len {
      let mut similarity = 0.0_f64;
      for (unit_index, unit) in query.units.iter().enumerate() {
        if unit.positions != query_positions {
          continue;
        }
        for form in &alias.forms {
          if form.form != unit.form {
            continue;
          }
          let id = form
            .tokens
            .get(listed)
            .expect("validated alias forms share positions")
            .id;
          for hit in matches {
            if position(hit.unit) == unit_index
              && hit.form == form.form
              && hit.id == id
            {
              similarity = similarity.max(hit.similarity);
            }
          }
        }
      }
      if similarity > 0.0 {
        insert(Pair {
          word: true,
          query: query_positions.iter().map(|&p| position(p)).collect(),
          listed: vec![listed],
          similarity,
        });
      }
    }
    for form in &alias.forms {
      for join in &form.joins {
        let mut similarity = 0.0_f64;
        for (unit_index, unit) in query.units.iter().enumerate() {
          if unit.positions != query_positions || unit.form != form.form {
            continue;
          }
          for hit in matches {
            if position(hit.unit) == unit_index
              && hit.form == form.form
              && hit.id == join.id
            {
              similarity = similarity.max(hit.similarity);
            }
          }
        }
        if similarity > 0.0 {
          insert(Pair {
            word: true,
            query: query_positions.iter().map(|&p| position(p)).collect(),
            listed: vec![position(join.left), position(join.right)],
            similarity,
          });
        }
      }
    }
  }
  append_initial_pairs(
    query,
    alias,
    &mut token_text,
    initial_similarity,
    &mut insert,
  );
  pairs
}

fn append_initial_pairs(
  query: &super::EntityQuery,
  alias: &super::AliasInput,
  token_text: &mut impl FnMut(super::FormId, super::TokenId) -> String,
  initial_similarity: f64,
  insert: &mut impl FnMut(Pair),
) {
  for (position, token) in query.tokens.iter().enumerate() {
    let query_initial = token.text.chars().count() == 1;
    for form in &alias.forms {
      if form.form != token.form {
        continue;
      }
      for (listed, listed_token) in form.tokens.iter().enumerate() {
        let text = token_text(form.form, listed_token.id);
        // The legacy listed-token check uses JavaScript string length (UTF-16).
        let listed_initial = text.encode_utf16().count() == 1;
        let similarity = if query_initial && text.starts_with(&token.text) {
          if listed_initial {
            1.0
          } else {
            initial_similarity
          }
        } else if !query_initial
          && listed_initial
          && token.text.starts_with(&text)
        {
          initial_similarity
        } else {
          continue;
        };
        insert(Pair {
          word: false,
          query: vec![position],
          listed: vec![listed],
          similarity,
        });
      }
    }
  }
}

/// Port the legacy candidate gate, including its single-word lexical-only path.
pub(super) fn candidate_estimate(
  query: &super::EntityQuery,
  alias: &super::AliasInput,
  matches: &[super::UnitMatch],
  token_text: impl FnMut(super::FormId, super::TokenId) -> String,
  initial_similarity: f64,
  quality: f64,
) -> f64 {
  let mut pairs =
    candidate_pairs(query, alias, matches, token_text, initial_similarity);
  let distinct_units = query
    .units
    .iter()
    .map(|unit| &unit.positions)
    .collect::<std::collections::BTreeSet<_>>()
    .len();
  if query.tokens.len() == 1
    && distinct_units == 1
    && !pairs.iter().any(|pair| pair.word && pair.listed.len() > 1)
  {
    pairs.retain(|pair| pair.word);
    let similarity = pairs
      .iter()
      .map(|pair| pair.similarity)
      .fold(0.0_f64, f64::max);
    let mut listed = BTreeMap::<usize, f64>::new();
    for pair in &pairs {
      for &position in &pair.listed {
        let maximum = listed.entry(position).or_default();
        *maximum = maximum.max(pair.similarity);
      }
    }
    return alias
      .forms
      .iter()
      .map(|form| {
        let explained: f64 = form
          .tokens
          .iter()
          .enumerate()
          .map(|(position, token)| {
            token.weight * listed.get(&position).copied().unwrap_or(0.0)
          })
          .sum();
        let minimum_total: f64 =
          form.tokens.iter().map(|token| token.minimum_weight).sum();
        (similarity * (explained / minimum_total).min(1.0)).sqrt() * quality
      })
      .fold(0.0_f64, f64::max);
  }
  let query_weights: Vec<_> =
    query.tokens.iter().map(|token| token.weight).collect();
  alias
    .forms
    .iter()
    .map(|form| {
      let listed_weights: Vec<_> =
        form.tokens.iter().map(|token| token.weight).collect();
      let minimum_weights: Vec<_> = form
        .tokens
        .iter()
        .map(|token| token.minimum_weight)
        .collect();
      bound(
        &pairs,
        &query_weights,
        &listed_weights,
        &minimum_weights,
        quality,
      )
    })
    .fold(0.0_f64, f64::max)
}

pub(super) fn score(
  pairs: &[Pair],
  query_weights: &[f64],
  listed_weights: &[f64],
  minimum_weights: &[f64],
  quality: f64,
) -> f64 {
  let mut ordered = pairs.to_vec();
  // Stable sorting preserves the candidate-generation order for exact ties.
  ordered.sort_by(|left, right| {
    right.similarity.total_cmp(&left.similarity).then_with(|| {
      right
        .query
        .len()
        .saturating_add(right.listed.len())
        .cmp(&left.query.len().saturating_add(left.listed.len()))
    })
  });
  let mut query = BTreeMap::new();
  let mut listed = BTreeMap::new();
  let mut anchored = false;
  for pair in ordered {
    if pair
      .query
      .iter()
      .any(|&position| query.contains_key(&position))
      || pair
        .listed
        .iter()
        .any(|&position| listed.contains_key(&position))
    {
      continue;
    }
    for position in pair.query {
      query.insert(position, pair.similarity);
    }
    for position in pair.listed {
      listed.insert(position, pair.similarity);
    }
    anchored |= pair.word;
  }
  if !anchored {
    return 0.0;
  }
  let query_total: f64 = query_weights.iter().sum();
  let query_explained: f64 = query_weights
    .iter()
    .enumerate()
    .map(|(position, weight)| {
      weight * query.get(&position).copied().unwrap_or(0.0)
    })
    .sum();
  let mut aligned_total = 0.0;
  let mut explained = 0.0;
  for (position, (weight, minimum)) in
    listed_weights.iter().zip(minimum_weights).enumerate()
  {
    let matched = listed.get(&position);
    aligned_total += if matched.is_some() { *weight } else { *minimum };
    if let Some(similarity) = matched {
      explained += weight * similarity;
    }
  }
  ((query_explained / query_total) * (explained / aligned_total).min(1.0))
    .sqrt()
    * quality
}

pub(super) fn bound(
  pairs: &[Pair],
  query_weights: &[f64],
  listed_weights: &[f64],
  minimum_weights: &[f64],
  quality: f64,
) -> f64 {
  if !pairs.iter().any(|pair| pair.word) {
    return 0.0;
  }
  let mut query = BTreeMap::<usize, f64>::new();
  let mut listed = BTreeMap::<usize, f64>::new();
  for pair in pairs {
    for &position in &pair.query {
      let maximum = query.entry(position).or_default();
      *maximum = maximum.max(pair.similarity);
    }
    for &position in &pair.listed {
      let maximum = listed.entry(position).or_default();
      *maximum = maximum.max(pair.similarity);
    }
  }
  let query_total: f64 = query_weights.iter().sum();
  let query_explained: f64 = query_weights
    .iter()
    .enumerate()
    .map(|(p, w)| w * query.get(&p).copied().unwrap_or(0.0))
    .sum();
  let minimum_total: f64 = minimum_weights.iter().sum();
  let listed_explained: f64 = listed_weights
    .iter()
    .enumerate()
    .map(|(p, w)| w * listed.get(&p).copied().unwrap_or(0.0))
    .sum();
  ((query_explained / query_total)
    * (listed_explained / minimum_total).min(1.0))
  .sqrt()
    * quality
}

#[cfg(test)]
mod tests {
  use super::{Pair, bound, score};
  use proptest::prelude::*;

  // These fixed fixtures always contain exactly one token and candidate.
  #[test]
  #[allow(clippy::expect_used)]
  fn astral_initial_retains_legacy_utf16_listed_semantics() {
    use super::super::{
      AliasForm, AliasInput, AliasToken, EntityQuery, QueryToken,
    };
    let alias = AliasInput {
      entry_id: 0,
      entity_group: 0,
      quality: 1.0,
      forms: vec![AliasForm {
        form: 0,
        tokens: vec![AliasToken {
          id: 0,
          weight: 1.0,
          minimum_weight: 1.0,
        }],
        joins: vec![],
        minimum_total: 1.0,
      }],
    };
    let mut query = EntityQuery {
      tokens: vec![QueryToken {
        form: 0,
        text: "𐐀".to_owned(),
        weight: 1.0,
      }],
      units: vec![],
    };
    let pairs =
      super::candidate_pairs(&query, &alias, &[], |_, _| "𐐀".to_owned(), 0.8);
    assert_eq!(pairs.len(), 1);
    assert!(
      (pairs.first().expect("one initial pair").similarity - 0.8).abs()
        < f64::EPSILON
    );
    query.tokens.first_mut().expect("one query token").text =
      "𐐀word".to_owned();
    let longer_pairs =
      super::candidate_pairs(&query, &alias, &[], |_, _| "𐐀".to_owned(), 0.8);
    assert!(
      longer_pairs.is_empty(),
      "an astral listed token has two UTF-16 units"
    );
  }

  #[test]
  fn single_word_estimate_ignores_stronger_initial_pair() {
    use super::super::{
      AliasForm, AliasInput, AliasToken, EntityQuery, QueryToken, QueryUnit,
      UnitMatch,
    };
    let query = EntityQuery {
      tokens: vec![QueryToken {
        form: 0,
        text: "alpha".to_owned(),
        weight: 1.0,
      }],
      units: vec![QueryUnit {
        form: 0,
        text: "alpha".to_owned(),
        positions: vec![0],
      }],
    };
    let alias = AliasInput {
      entry_id: 0,
      entity_group: 0,
      quality: 1.0,
      forms: vec![AliasForm {
        form: 0,
        tokens: vec![
          AliasToken {
            id: 0,
            weight: 1.0,
            minimum_weight: 1.0,
          },
          AliasToken {
            id: 1,
            weight: 1.0,
            minimum_weight: 1.0,
          },
        ],
        joins: vec![],
        minimum_total: 2.0,
      }],
    };
    let matches = vec![UnitMatch {
      unit: 0,
      form: 0,
      id: 0,
      similarity: 0.75,
    }];
    let text = |_, id| {
      if id == 0 {
        "alfa".to_owned()
      } else {
        "a".to_owned()
      }
    };
    let estimate =
      super::candidate_estimate(&query, &alias, &matches, text, 0.8, 1.0);
    let expected = (0.75_f64 * (0.75 / 2.0)).sqrt();
    assert!((estimate - expected).abs() < 1e-12);
    let pairs = super::candidate_pairs(&query, &alias, &matches, text, 0.8);
    let generic = bound(&pairs, &[1.0], &[1.0, 1.0], &[1.0, 1.0], 1.0);
    assert!(
      generic > estimate,
      "the generic maximum includes the stronger initial"
    );
  }

  #[test]
  fn single_word_estimate_does_not_multiply_subnormal_query_weight() {
    use super::super::{
      AliasForm, AliasInput, AliasToken, EntityQuery, QueryToken, QueryUnit,
      UnitMatch,
    };
    let tiny = f64::from_bits(1);
    let query = EntityQuery {
      tokens: vec![QueryToken {
        form: 0,
        text: "alhpa".to_owned(),
        weight: tiny,
      }],
      units: vec![QueryUnit {
        form: 0,
        text: "alhpa".to_owned(),
        positions: vec![0],
      }],
    };
    let alias = AliasInput {
      entry_id: 0,
      entity_group: 0,
      quality: 1.0,
      forms: vec![AliasForm {
        form: 0,
        tokens: vec![AliasToken {
          id: 0,
          weight: tiny,
          minimum_weight: tiny,
        }],
        joins: vec![],
        minimum_total: tiny,
      }],
    };
    let matches = vec![UnitMatch {
      unit: 0,
      form: 0,
      id: 0,
      similarity: 0.8,
    }];
    let estimate = super::candidate_estimate(
      &query,
      &alias,
      &matches,
      |_, _| "alpha".to_owned(),
      0.8,
      1.0,
    );
    assert!((estimate - 0.8_f64.sqrt()).abs() < f64::EPSILON);
    let pairs = super::candidate_pairs(
      &query,
      &alias,
      &matches,
      |_, _| "alpha".to_owned(),
      0.8,
    );
    assert!(
      (score(&pairs, &[tiny], &[tiny], &[tiny], 1.0) - 1.0).abs()
        < f64::EPSILON
    );
  }

  proptest! {
    #[test]
    #[allow(clippy::expect_used)] // The generated weight vector is nonempty.
    fn optimistic_estimate_dominates_greedy_alignment(
      weights in prop::collection::vec((1_u16..1000, 1_u16..1000), 1..12),
      candidates in prop::collection::vec((any::<bool>(), any::<u8>(), any::<u8>(), 0_u16..=1000, any::<bool>()), 0..50),
      quality in 0_u16..=1000,
    ) {
      let query_weights: Vec<_> = weights.iter().map(|&(weight, _)| f64::from(weight)).collect();
      let listed_weights = query_weights.clone();
      let minimum_weights: Vec<_> = weights.iter().map(|&(weight, fraction)| f64::from(weight) * f64::from(fraction) / 1000.0).collect();
      let pairs: Vec<_> = candidates.into_iter().map(|(word, q, l, similarity, join)| {
        let q = usize::from(q).checked_rem(weights.len()).expect("nonempty weights");
        let l = usize::from(l).checked_rem(weights.len()).expect("nonempty weights");
        Pair {
          word,
          query: if join && q.saturating_add(1) < weights.len() { vec![q, q.saturating_add(1)] } else { vec![q] },
          listed: if join && l.saturating_add(1) < weights.len() { vec![l, l.saturating_add(1)] } else { vec![l] },
          similarity: f64::from(similarity) / 1000.0,
        }
      }).collect();
      let quality = f64::from(quality) / 1000.0;
      let actual = score(&pairs, &query_weights, &listed_weights, &minimum_weights, quality);
      let optimistic = bound(&pairs, &query_weights, &listed_weights, &minimum_weights, quality);
      prop_assert!(optimistic + 1e-12 >= actual, "bound {optimistic} < score {actual}");
    }
  }
}
