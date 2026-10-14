//! Independent test port of the legacy TypeScript alias scorer and estimate.
//!
//! Keep the greedy alignment and coverage arithmetic explicit here so changes to
//! retrieval do not silently change the reference semantics.

#[derive(Clone, Debug)]
pub(super) struct Pair {
  pub word: bool,
  pub query: Vec<usize>,
  pub listed: Vec<usize>,
  pub similarity: f64,
}

/// Build candidates independently from the retrieval implementation. Forms sharing
/// positions represent alternate spellings; their similarities combine by maximum.
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
          let id = form.tokens[listed].id;
          for hit in matches {
            if hit.unit as usize == unit_index
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
          query: query_positions.iter().map(|&p| p as usize).collect(),
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
            if hit.unit as usize == unit_index
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
            query: query_positions.iter().map(|&p| p as usize).collect(),
            listed: vec![join.left as usize, join.right as usize],
            similarity,
          });
        }
      }
    }
  }
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
  pairs
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
      (right.query.len() + right.listed.len())
        .cmp(&(left.query.len() + left.listed.len()))
    })
  });
  let mut query = vec![None; query_weights.len()];
  let mut listed = vec![None; listed_weights.len()];
  let mut anchored = false;
  for pair in ordered {
    if pair.query.iter().any(|&position| query[position].is_some())
      || pair
        .listed
        .iter()
        .any(|&position| listed[position].is_some())
    {
      continue;
    }
    for position in pair.query {
      query[position] = Some(pair.similarity);
    }
    for position in pair.listed {
      listed[position] = Some(pair.similarity);
    }
    anchored |= pair.word;
  }
  if !anchored {
    return 0.0;
  }
  let query_total: f64 = query_weights.iter().sum();
  let query_explained: f64 = query_weights
    .iter()
    .zip(query)
    .map(|(weight, matched)| weight * matched.unwrap_or(0.0))
    .sum();
  let mut aligned_total = 0.0;
  let mut explained = 0.0;
  for ((weight, minimum), matched) in
    listed_weights.iter().zip(minimum_weights).zip(listed)
  {
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
  let mut query = vec![0.0_f64; query_weights.len()];
  let mut listed = vec![0.0_f64; listed_weights.len()];
  for pair in pairs {
    for &position in &pair.query {
      query[position] = query[position].max(pair.similarity);
    }
    for &position in &pair.listed {
      listed[position] = listed[position].max(pair.similarity);
    }
  }
  let query_total: f64 = query_weights.iter().sum();
  let query_explained: f64 =
    query_weights.iter().zip(query).map(|(w, s)| w * s).sum();
  let minimum_total: f64 = minimum_weights.iter().sum();
  let listed_explained: f64 =
    listed_weights.iter().zip(listed).map(|(w, s)| w * s).sum();
  ((query_explained / query_total)
    * (listed_explained / minimum_total).min(1.0))
  .sqrt()
    * quality
}

#[cfg(test)]
mod tests {
  use super::{Pair, bound, score};
  use proptest::prelude::*;

  #[test]
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
    assert!((pairs[0].similarity - 0.8).abs() < f64::EPSILON);
    query.tokens[0].text = "𐐀word".to_owned();
    let pairs =
      super::candidate_pairs(&query, &alias, &[], |_, _| "𐐀".to_owned(), 0.8);
    assert!(
      pairs.is_empty(),
      "an astral listed token has two UTF-16 units"
    );
  }

  proptest! {
    #[test]
    fn optimistic_estimate_dominates_greedy_alignment(
      weights in prop::collection::vec((1_u16..1000, 1_u16..1000), 1..12),
      candidates in prop::collection::vec((any::<bool>(), any::<u8>(), any::<u8>(), 0_u16..=1000, any::<bool>()), 0..50),
      quality in 0_u16..=1000,
    ) {
      let query_weights: Vec<_> = weights.iter().map(|&(weight, _)| f64::from(weight)).collect();
      let listed_weights = query_weights.clone();
      let minimum_weights: Vec<_> = weights.iter().map(|&(weight, fraction)| f64::from(weight) * f64::from(fraction) / 1000.0).collect();
      let pairs: Vec<_> = candidates.into_iter().map(|(word, q, l, similarity, join)| {
        let q = usize::from(q) % weights.len();
        let l = usize::from(l) % weights.len();
        Pair {
          word,
          query: if join && q + 1 < weights.len() { vec![q, q + 1] } else { vec![q] },
          listed: if join && l + 1 < weights.len() { vec![l, l + 1] } else { vec![l] },
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
