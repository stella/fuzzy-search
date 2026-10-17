//! Frontier completeness through real posting projection and exhaustive scoring.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::super::oracle;
use super::super::{
  AliasForm, AliasInput, AliasJoin, AliasToken, BudgetBasis, EditBudget,
  EntityIndex, EntityIndexBuilder, EntityIndexOptions, EntityQuery, QueryToken,
  QueryUnit, SearchParams,
};
use super::{
  Cost, SearchIndex, alias_bound, exact, frontier_bound, prepare, upper_product,
};

const WORDS: [&str; 8] =
  ["alpha", "alhpa", "beta", "betas", "a", "日", "日本", "𐐀"];

fn text(id: u8, form: u32) -> String {
  let word = WORDS.get(usize::from(id) % WORDS.len()).unwrap();
  if form == 0 {
    (*word).to_owned()
  } else {
    word.to_uppercase()
  }
}

// Always exercise self, reversed and nonconsecutive joins. Caller-supplied
// join text is deliberately independent of position order, as the API permits.
fn joins(count: usize) -> Vec<(u32, u32)> {
  let last = u32::try_from(count - 1).unwrap();
  vec![(0, 0), (last, 0), (0, last)]
}

fn fixture(rows: &[Vec<(u8, u16, u8)>], qualities: &[u16]) -> EntityIndex {
  let mut builder = EntityIndexBuilder::new(EntityIndexOptions {
    forms: 2,
    budget: EditBudget {
      by_length: vec![0, 0, 0, 1, 2],
      basis: BudgetBasis::Shorter,
    },
    initial_weight: 0.25,
    initial_similarity: 0.8,
  })
  .unwrap();
  let mut counts = [0_usize; 2];
  for (id, row) in rows.iter().enumerate() {
    let mut forms = Vec::new();
    for form in 0..2 {
      let mut tokens = Vec::new();
      for &(word, weight, discount) in row {
        let token_id = builder.intern(form, &text(word, form)).unwrap();
        let count = counts.get_mut(usize::try_from(form).unwrap()).unwrap();
        *count = (*count).max(usize::try_from(token_id).unwrap() + 1);
        tokens.push(AliasToken {
          id: token_id,
          weight: f64::from(weight),
          minimum_weight: f64::from(weight) * (f64::from(discount) + 1.0)
            / 256.0,
        });
      }
      let mut alias_joins = Vec::new();
      for (left, right) in joins(row.len()) {
        let joined = format!(
          "{}{}",
          text(row.get(usize::try_from(left).unwrap()).unwrap().0, form),
          text(row.get(usize::try_from(right).unwrap()).unwrap().0, form)
        );
        let join_id = builder.intern(form, &joined).unwrap();
        let count = counts.get_mut(usize::try_from(form).unwrap()).unwrap();
        *count = (*count).max(usize::try_from(join_id).unwrap() + 1);
        alias_joins.push(AliasJoin {
          id: join_id,
          left,
          right,
        });
      }
      forms.push(AliasForm {
        form,
        minimum_total: tokens.iter().map(|token| token.minimum_weight).sum(),
        tokens,
        joins: alias_joins,
      });
    }
    builder
      .add_alias(AliasInput {
        entry_id: u32::try_from(id).unwrap(),
        entity_group: 0,
        quality: f64::from(*qualities.get(id % qualities.len()).unwrap())
          / 1000.0,
        forms,
      })
      .unwrap();
  }
  while !builder.finish_step(100).unwrap() {}
  // The search preserves query weights; these tables only validate the stored
  // vocabulary and are not consulted for the caller-prepared test queries.
  builder
    .finish(counts.into_iter().map(|count| vec![1.0; count]).collect())
    .unwrap()
}

fn query(ids: &[u8], weights: &[u16]) -> EntityQuery {
  let tokens = ids
    .iter()
    .enumerate()
    .map(|(position, id)| QueryToken {
      form: 0,
      text: text(*id, 0),
      weight: f64::from(*weights.get(position % weights.len()).unwrap()),
    })
    .collect::<Vec<_>>();
  let mut units = Vec::new();
  for form in 0..2 {
    for (position, id) in ids.iter().enumerate() {
      units.push(QueryUnit {
        form,
        text: text(*id, form),
        positions: vec![u32::try_from(position).unwrap()],
      });
    }
    for (left, right) in joins(ids.len()) {
      let left_id = *ids.get(usize::try_from(left).unwrap()).unwrap();
      let right_id = *ids.get(usize::try_from(right).unwrap()).unwrap();
      let positions = if left == right {
        vec![left]
      } else {
        vec![left, right]
      };
      units.push(QueryUnit {
        form,
        text: format!("{}{}", text(left_id, form), text(right_id, form)),
        positions,
      });
    }
  }
  EntityQuery { tokens, units }
}

fn check(
  index: &EntityIndex,
  query: &EntityQuery,
  quality: f64,
  sampled_tau: f64,
) {
  let (mut prepared, exhausted) =
    prepare(index, query, &mut Cost::default(), u64::MAX).unwrap();
  assert!(!exhausted);
  let estimates = index
    .aliases
    .iter()
    .map(|alias| {
      oracle::candidate_estimate(
        query,
        alias,
        &prepared.matches,
        |form, id| index.lexicon(form).unwrap().token(id).unwrap().to_owned(),
        index.options.initial_similarity,
        alias.quality,
      )
    })
    .collect::<Vec<_>>();
  let weights = query
    .tokens
    .iter()
    .map(|token| token.weight)
    .collect::<Vec<_>>();
  let scores = index
    .aliases
    .iter()
    .map(|alias| {
      if exact(index, query, alias).unwrap() {
        return alias.quality;
      }
      let pairs = oracle::candidate_pairs(
        query,
        alias,
        &prepared.matches,
        |form, id| index.lexicon(form).unwrap().token(id).unwrap().to_owned(),
        index.options.initial_similarity,
      );
      let form = alias.forms.first().unwrap();
      oracle::score(
        &pairs,
        &weights,
        &form
          .tokens
          .iter()
          .map(|token| token.weight)
          .collect::<Vec<_>>(),
        &form
          .tokens
          .iter()
          .map(|token| token.minimum_weight)
          .collect::<Vec<_>>(),
        alias.quality,
      )
    })
    .collect::<Vec<_>>();
  let mut seen = BTreeSet::new();
  loop {
    let bound = frontier_bound(index, query, &prepared.cursors).unwrap();
    for (id, estimate) in estimates.iter().enumerate() {
      if seen.contains(&u32::try_from(id).unwrap()) {
        continue;
      }
      assert!(
        bound + 1e-12 >= *estimate,
        "frontier {bound} < unseen oracle estimate {estimate}; query {query:?}"
      );
      let score = scores.get(id).unwrap();
      assert!(
        bound + 1e-12 >= *score,
        "frontier {bound} < unseen oracle score {score}"
      );
    }
    let mut advanced = false;
    for cursor in &mut prepared.cursors {
      if let Some((id, _)) = index.posting(cursor.list, cursor.offset) {
        seen.insert(id);
        cursor.offset += 1;
        advanced = true;
      }
    }
    if !advanced {
      break;
    }
  }
  for tau in [0.0, sampled_tau, 0.25, 0.5, 0.75, 1.0] {
    let result = index
      .search(
        query,
        &SearchParams {
          tau,
          quality,
          max_cost: u64::MAX,
        },
      )
      .unwrap();
    assert!(!result.exhausted);
    let exhaustive = index
      .aliases
      .iter()
      .enumerate()
      .filter_map(|(id, alias)| {
        let bound = if exact(index, query, alias).unwrap() {
          1.0
        } else {
          alias_bound(index, query, &prepared, alias).unwrap()
        };
        (tau <= 0.0
          || upper_product(upper_product(bound, alias.quality), quality)
            + 1e-12
            >= tau)
          .then(|| u32::try_from(id).unwrap())
      })
      .collect::<Vec<_>>();
    assert_eq!(
      result.survivors, exhaustive,
      "threshold {tau}; query {query:?}"
    );
  }
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(48))]
  #[test]
  fn frontier_dominates_unseen_oracle_and_matches_exhaustive(
    rows in prop::collection::vec(prop::collection::vec((any::<u8>(), 1_u16..100, any::<u8>()), 3..6), 1..10),
    ids in prop::collection::vec(any::<u8>(), 3..6),
    weights in prop::collection::vec(1_u16..100, 1..6),
    qualities in prop::collection::vec(0_u16..=1000, 1..10),
    quality in 0_u16..=1000,
    tau in 0_u16..=1000,
  ) {
    check(&fixture(&rows, &qualities), &query(&ids, &weights), f64::from(quality) / 1000.0, f64::from(tau) / 1000.0);
  }
}

#[test]
fn duplicated_forms_units_and_repeated_positions_preserve_threshold_boundaries()
{
  let rows = vec![
    vec![(0, 1, 255), (0, 1, 255), (2, 1, 255)],
    vec![(0, 1, 255), (1, 1, 255), (2, 1, 255)],
    vec![(4, 1, 255), (2, 1, 255), (0, 1, 255)],
    vec![(2, 1, 255), (0, 1, 255), (2, 1, 255)],
  ];
  let index = fixture(&rows, &[1000, 750, 0, 500]);
  for ids in [&[0, 0, 2][..], &[0, 2, 0][..], &[4, 0, 2][..]] {
    let mut query = query(ids, &[1]);
    query.units.extend(query.units.clone());
    for quality in [0.0, 0.5, 1.0] {
      check(&index, &query, quality, 0.8);
    }
  }
}

#[test]
fn one_query_position_can_explain_multiple_listed_positions() {
  let index = fixture(&[vec![(0, 1, 255), (1, 1, 255), (2, 1, 255)]], &[1000]);
  let query = query(&[0], &[1]);
  let (prepared, exhausted) =
    prepare(&index, &query, &mut Cost::default(), u64::MAX).unwrap();
  assert!(!exhausted);
  let alias = index.aliases.first().unwrap();
  let estimate = oracle::candidate_estimate(
    &query,
    alias,
    &prepared.matches,
    |form, id| index.lexicon(form).unwrap().token(id).unwrap().to_owned(),
    index.options.initial_similarity,
    1.0,
  );
  // Both alpha and alhpa contribute to listed coverage for the same query
  // position; taking only the largest posting impact would understate it.
  assert!(estimate > (1.0_f64 / 3.0).sqrt());
  check(&index, &query, 1.0, estimate);
}
