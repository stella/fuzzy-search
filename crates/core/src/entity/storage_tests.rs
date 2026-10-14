//! Builder-to-storage invariants, exercised through the real construction path.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use proptest::prelude::*;

use super::search::PostingList;
use super::{
  AliasForm, AliasInput, AliasJoin, AliasToken, BudgetBasis, EditBudget,
  EntityIndex, EntityIndexBuilder, EntityIndexOptions, EntityQuery, QueryToken,
  QueryUnit, SearchParams,
};

fn options(forms: u32) -> EntityIndexOptions {
  EntityIndexOptions {
    forms,
    budget: EditBudget {
      by_length: vec![0, 0, 1],
      basis: BudgetBasis::Shorter,
    },
    initial_weight: 1.0,
    initial_similarity: 0.8,
  }
}

fn alias(ids: &[u32]) -> AliasInput {
  AliasInput {
    entry_id: 0,
    entity_group: 0,
    quality: 1.0,
    forms: vec![AliasForm {
      form: 0,
      tokens: ids
        .iter()
        .map(|&id| AliasToken {
          id,
          weight: 2.0,
          minimum_weight: 1.0,
        })
        .collect(),
      joins: Vec::new(),
      minimum_total: f64::from(u32::try_from(ids.len()).unwrap()),
    }],
  }
}

fn decoded(index: &EntityIndex, list: PostingList) -> Vec<(u32, f64)> {
  (0..)
    .map_while(|offset| index.postings.get(list, offset))
    .collect()
}

fn assert_impacts(index: &EntityIndex, list: PostingList, raw: &[f64]) {
  let entries = decoded(index, list);
  assert_eq!(entries.len(), raw.len());
  for &(id, impact) in &entries {
    let expected = raw.get(usize::try_from(id).unwrap()).unwrap();
    assert!(
      impact >= *expected,
      "decoded {impact} is below raw {expected}"
    );
    assert!(impact <= expected + 0.001, "quantization must remain tight");
  }
  for pair in entries.windows(2) {
    if let [left, right] = pair {
      assert!(
        left.1 > right.1
          || (left.1.total_cmp(&right.1).is_eq() && left.0 < right.0)
      );
    }
  }
}

#[test]
fn projects_all_list_kinds_and_combines_duplicate_positions() {
  let mut builder = EntityIndexBuilder::new(options(1)).unwrap();
  let alpha = builder.intern(0, "alpha").unwrap();
  let initial = builder.intern(0, "a").unwrap();
  let joined = builder.intern(0, "alphaalpha").unwrap();
  let mut repeated = alias(&[alpha, alpha]);
  repeated.forms.first_mut().unwrap().joins.push(AliasJoin {
    id: joined,
    left: 0,
    right: 1,
  });
  builder.add_alias(repeated).unwrap();
  builder.add_alias(alias(&[initial, initial])).unwrap();
  let index = builder.finish(vec![vec![1.0; 3]]).unwrap();
  assert_eq!(
    decoded(&index, PostingList::Token { form: 0, id: alpha }).len(),
    1
  );
  assert_impacts(
    &index,
    PostingList::QueryInitial {
      form: 0,
      prefix: 'a',
    },
    &[1.6, 2.0],
  );
  for (list, expected_alias, raw) in [
    (PostingList::Token { form: 0, id: alpha }, 0, 2.0),
    (
      PostingList::Join {
        form: 0,
        id: joined,
      },
      0,
      2.0,
    ),
    (
      PostingList::ListedInitial {
        form: 0,
        prefix: u16::from(b'a'),
      },
      1,
      1.6,
    ),
  ] {
    let entries = decoded(&index, list);
    assert_eq!(entries.len(), 1);
    let &(id, impact) = entries.first().unwrap();
    assert_eq!(id, expected_alias);
    assert!(impact >= raw && impact <= raw + 0.001);
  }
  assert_eq!(index.posting_count(), 6);
}

#[test]
fn astral_scalar_is_not_a_listed_utf16_initial() {
  let mut builder = EntityIndexBuilder::new(options(1)).unwrap();
  let astral = builder.intern(0, "😀").unwrap();
  builder.add_alias(alias(&[astral])).unwrap();
  let index = builder.finish(vec![vec![1.0]]).unwrap();
  assert_impacts(
    &index,
    PostingList::QueryInitial {
      form: 0,
      prefix: '😀',
    },
    &[2.0],
  );
  let high_surrogate = "😀".encode_utf16().next().unwrap();
  assert!(
    decoded(
      &index,
      PostingList::ListedInitial {
        form: 0,
        prefix: high_surrogate
      }
    )
    .is_empty()
  );
  assert_eq!(index.posting_count(), 2);
}

fn shared_fixture() -> EntityIndexBuilder {
  let mut builder = EntityIndexBuilder::new(options(2)).unwrap();
  for form in 0..2 {
    for (expected, text) in ["zeta", "alpha", "東京"].into_iter().enumerate()
    {
      let id = builder.intern(form, text).unwrap();
      assert_eq!(id, u32::try_from(expected).unwrap());
      assert_eq!(builder.intern(form, text).unwrap(), id);
    }
  }
  builder.add_alias(alias(&[0, 1, 0])).unwrap();
  builder
}

#[test]
fn stepping_freezes_input_and_matches_sync_bytes_with_shared_caller_order_vocabularies()
 {
  let weights = vec![vec![1.0; 3]; 2];
  let sync = shared_fixture().finish(weights.clone()).unwrap();
  assert!(Arc::ptr_eq(
    sync.lexicons.first().unwrap(),
    sync.lexicons.get(1).unwrap()
  ));
  assert_eq!(sync.lexicons.first().unwrap().token(0), Some("zeta"));
  assert_eq!(sync.lexicons.first().unwrap().token(1), Some("alpha"));
  let mut builder = shared_fixture();
  assert!(!builder.finish_step(1).unwrap());
  assert!(builder.intern(0, "late").is_err());
  assert!(builder.add_alias(alias(&[0])).is_err());
  while !builder.finish_step(1).unwrap() {}
  assert!(builder.finish_step(1).unwrap());
  let stepped = builder.finish(weights).unwrap();
  assert_eq!(sync.to_bytes().unwrap(), stepped.to_bytes().unwrap());
  let restored = EntityIndex::from_bytes(&sync.to_bytes().unwrap()).unwrap();
  assert!(Arc::ptr_eq(
    restored.lexicons.first().unwrap(),
    restored.lexicons.get(1).unwrap()
  ));
}

#[test]
fn rejects_invalid_options_and_interning() {
  assert!(EntityIndexBuilder::new(options(0)).is_err());
  for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
    let mut invalid = options(1);
    invalid.initial_weight = value;
    assert!(EntityIndexBuilder::new(invalid).is_err());
  }
  for value in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
    let mut invalid = options(1);
    invalid.initial_similarity = value;
    assert!(EntityIndexBuilder::new(invalid).is_err());
  }
  let mut builder = EntityIndexBuilder::new(options(1)).unwrap();
  assert!(builder.finish_step(0).is_err());
  assert!(builder.intern(0, "").is_err());
  assert!(builder.intern(1, "alpha").is_err());
  assert!(builder.intern(0, &"a".repeat(0x0001_0000)).is_err());
  builder.intern(0, "alpha").unwrap();
}

#[test]
fn rejects_invalid_aliases_and_weight_tables() {
  let mut builder = EntityIndexBuilder::new(options(1)).unwrap();
  builder.intern(0, "alpha").unwrap();
  let valid = alias(&[0, 0]);
  let mut invalids = Vec::new();
  let mut missing_forms = valid.clone();
  missing_forms.forms.clear();
  invalids.push(missing_forms);
  for weight in [0.0, -1.0, f64::NAN, f64::INFINITY] {
    let mut invalid = valid.clone();
    invalid
      .forms
      .first_mut()
      .unwrap()
      .tokens
      .first_mut()
      .unwrap()
      .weight = weight;
    invalids.push(invalid);
  }
  for quality in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
    let mut invalid = valid.clone();
    invalid.quality = quality;
    invalids.push(invalid);
  }
  for value in [0.0, -1.0, f64::NAN, f64::INFINITY, 3.0] {
    let mut invalid = valid.clone();
    invalid
      .forms
      .first_mut()
      .unwrap()
      .tokens
      .first_mut()
      .unwrap()
      .minimum_weight = value;
    invalids.push(invalid);
    let mut invalid_total = valid.clone();
    invalid_total.forms.first_mut().unwrap().minimum_total = value;
    invalids.push(invalid_total);
  }
  for join in [
    AliasJoin {
      id: 1,
      left: 0,
      right: 1,
    },
    AliasJoin {
      id: 0,
      left: 1,
      right: 0,
    },
    AliasJoin {
      id: 0,
      left: 1,
      right: 2,
    },
  ] {
    let mut invalid = valid.clone();
    invalid.forms.first_mut().unwrap().joins.push(join);
    invalids.push(invalid);
  }
  let mut duplicate_form = valid.clone();
  duplicate_form.forms.extend(valid.forms.clone());
  invalids.push(duplicate_form);
  let mut missing_token = valid.clone();
  missing_token
    .forms
    .first_mut()
    .unwrap()
    .tokens
    .first_mut()
    .unwrap()
    .id = 1;
  invalids.push(missing_token);
  for invalid in invalids {
    assert!(builder.add_alias(invalid).is_err());
  }
  assert_eq!(builder.add_alias(valid).unwrap(), 0);
  for weights in [
    Vec::new(),
    vec![Vec::new(), Vec::new()],
    vec![vec![0.0; 3]; 2],
    vec![vec![f64::NAN; 3]; 2],
    vec![vec![f64::INFINITY; 3]; 2],
  ] {
    assert!(shared_fixture().finish(weights).is_err());
  }
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]
  #[test]
  fn actual_builder_postings_bound_accumulated_duplicate_weights(
    weights in prop::collection::vec(prop::collection::vec(1_u16..1000, 1..8), 1..20),
  ) {
    let mut builder = EntityIndexBuilder::new(options(1)).unwrap();
    let id = builder.intern(0, "alpha").unwrap();
    let mut raw = Vec::new();
    for positions in weights {
      let mut input = alias(&vec![id; positions.len()]);
      let form = input.forms.first_mut().unwrap();
      form.minimum_total = 1.0;
      for (token, weight) in form.tokens.iter_mut().zip(&positions) { token.weight = f64::from(*weight); }
      raw.push(positions.iter().map(|&weight| f64::from(weight)).sum::<f64>());
      builder.add_alias(input).unwrap();
    }
    let index = builder.finish(vec![vec![1.0]]).unwrap();
    let entries = decoded(&index, PostingList::Token { form: 0, id });
    prop_assert_eq!(entries.len(), raw.len());
    for &(alias, impact) in &entries { prop_assert!(impact >= *raw.get(usize::try_from(alias).unwrap()).unwrap()); }
    for pair in entries.windows(2) {
      if let [left, right] = pair { prop_assert!(left.1 > right.1 || (left.1.total_cmp(&right.1).is_eq() && left.0 < right.0)); }
    }
  }
}

#[test]
#[ignore = "explicit 100k-alias allocation and peak-RSS measurement"]
#[allow(clippy::print_stdout, clippy::disallowed_macros)] // Explicit measurement output for the ignored memory fixture.
fn memory_100k_aliases_two_forms() {
  let mut builder = EntityIndexBuilder::new(options(2)).unwrap();
  for form in 0..2 {
    builder.intern(form, "company").unwrap();
    builder.intern(form, "limited").unwrap();
    for number in 0..100_000 {
      builder.intern(form, &format!("{number:06}")).unwrap();
    }
  }
  for number in 0..100_000_u32 {
    let mut input = alias(&[0, 1, number.checked_add(2).unwrap()]);
    input.entry_id = number;
    input.entity_group = number;
    let mut second = input.forms.first().unwrap().clone();
    second.form = 1;
    input.forms.push(second);
    builder.add_alias(input).unwrap();
  }
  let index = builder.finish(vec![vec![1.0; 100_002]; 2]).unwrap();
  assert_eq!(index.aliases.len(), 100_000);
  assert!(Arc::ptr_eq(
    index.lexicons.first().unwrap(),
    index.lexicons.get(1).unwrap()
  ));
  println!(
    "entity memory fixture: aliases=100000 forms=2 native_bytes={} posting_count={}",
    index.native_bytes(),
    index.posting_count()
  );
}

fn assert_oracle_completeness(
  index: &EntityIndex,
  query: &EntityQuery,
  params: &SearchParams,
) {
  let result = index.search(query, params).unwrap();
  assert!(!result.exhausted);
  let restored = EntityIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
  assert_eq!(result, restored.search(query, params).unwrap());
  let query_weights = query
    .tokens
    .iter()
    .map(|token| token.weight)
    .collect::<Vec<_>>();
  let lexicon = index.lexicons.first().unwrap();
  for (alias_id, input) in index.aliases.iter().enumerate() {
    let pairs = super::oracle::candidate_pairs(
      query,
      input,
      &result.unit_matches,
      |_, token_id| lexicon.token(token_id).unwrap().to_owned(),
      index.options.initial_similarity,
    );
    let form = input.forms.first().unwrap();
    let listed_weights = form
      .tokens
      .iter()
      .map(|token| token.weight)
      .collect::<Vec<_>>();
    let minimum_weights = form
      .tokens
      .iter()
      .map(|token| token.minimum_weight)
      .collect::<Vec<_>>();
    let mut query_text = query
      .tokens
      .iter()
      .map(|token| token.text.as_str())
      .collect::<Vec<_>>();
    let mut listed_text = form
      .tokens
      .iter()
      .map(|token| lexicon.token(token.id).unwrap())
      .collect::<Vec<_>>();
    query_text.sort_unstable();
    listed_text.sort_unstable();
    let score = if query_text == listed_text {
      input.quality
    } else {
      super::oracle::score(
        &pairs,
        &query_weights,
        &listed_weights,
        &minimum_weights,
        input.quality,
      )
    };
    if score * params.quality >= params.tau {
      assert!(
        result.survivors.contains(&u32::try_from(alias_id).unwrap()),
        "actual storage lost alias {alias_id}: score={score}, params={params:?}, query={query:?}"
      );
    }
  }
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]
  #[test]
  fn actual_builder_search_is_complete_against_reference_scorer(
    rows in prop::collection::vec((prop::collection::vec(0_u32..3, 1..5), 0_u16..=1000), 1..14),
    query_ids in prop::collection::vec(0_u32..3, 1..4),
    cutoff in 0_u16..=1000,
    quality in 0_u16..=1000,
  ) {
    // One explicit weight model: vocabulary/query/alias positions all weigh one;
    // unmatched alias positions retain a one-half minimum weight.
    let texts = ["alpha", "a", "beta"];
    let mut builder = EntityIndexBuilder::new(options(1)).unwrap();
    for text in texts { builder.intern(0, text).unwrap(); }
    let joined = builder.intern(0, "alphabeta").unwrap();
    for (ids, alias_quality) in rows {
      let mut input = alias(&ids);
      input.quality = f64::from(alias_quality) / 1000.0;
      let form = input.forms.first_mut().unwrap();
      for token in &mut form.tokens { token.weight = 1.0; token.minimum_weight = 0.5; }
      form.minimum_total = f64::from(u32::try_from(ids.len()).unwrap()) * 0.5;
      for (position, pair) in ids.windows(2).enumerate() {
        if pair == [0, 2] {
          form.joins.push(AliasJoin { id: joined, left: u32::try_from(position).unwrap(), right: u32::try_from(position.saturating_add(1)).unwrap() });
        }
      }
      builder.add_alias(input).unwrap();
    }
    let tokens = query_ids.iter().map(|id| QueryToken { form: 0, text: (*texts.get(usize::try_from(*id).unwrap()).unwrap()).to_owned(), weight: 1.0 }).collect::<Vec<_>>();
    let mut units = tokens.iter().enumerate().map(|(position, token)| QueryUnit { form: 0, text: token.text.clone(), positions: vec![u32::try_from(position).unwrap()] }).collect::<Vec<_>>();
    for (position, pair) in tokens.windows(2).enumerate() {
      if let [left, right] = pair { units.push(QueryUnit { form: 0, text: format!("{}{}", left.text, right.text), positions: vec![u32::try_from(position).unwrap(), u32::try_from(position.saturating_add(1)).unwrap()] }); }
    }
    let index = builder.finish(vec![vec![1.0; 4]]).unwrap();
    assert_oracle_completeness(&index, &EntityQuery { tokens, units }, &SearchParams { tau: f64::from(cutoff) / 1000.0, quality: f64::from(quality) / 1000.0, max_cost: u64::MAX });
  }
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]
  #[test]
  fn rejects_crossed_form_position_models(left in 1_u16..400, right in 401_u16..800) {
    for crossed_minimums in [false, true] {
      let mut builder = EntityIndexBuilder::new(options(2)).unwrap();
      for form in 0..2 { builder.intern(form, "alpha").unwrap(); builder.intern(form, "beta").unwrap(); }
      let mut input = alias(&[0, 1]);
      let reference = input.forms.first_mut().unwrap();
      reference.minimum_total = 1.0;
      for (token, value) in reference.tokens.iter_mut().zip([left, right]) {
        token.weight = if crossed_minimums { 1000.0 } else { f64::from(value) };
        token.minimum_weight = if crossed_minimums { f64::from(value) } else { 1.0 };
      }
      let mut second = reference.clone();
      second.form = 1;
      input.forms.push(second);
      // The baseline must pass all unrelated id, count, and numeric checks.
      prop_assert_eq!(builder.add_alias(input.clone()).unwrap(), 0);
      let other_form = input.forms.get_mut(1).unwrap();
      if crossed_minimums {
        for (token, value) in other_form.tokens.iter_mut().zip([right, left]) { token.minimum_weight = f64::from(value); }
      } else {
        for (token, value) in other_form.tokens.iter_mut().zip([right, left]) { token.weight = f64::from(value); }
      }
      prop_assert!(builder.add_alias(input).is_err());
    }
  }

  #[test]
  fn initial_posting_share_is_scale_invariant(
    bits in prop_oneof![Just(1_u64), Just(0x0010_0000_0000_0000_u64), Just(0x7FEF_FFFF_FFFF_FFFF_u64), 1_u64..=0x7FEF_FFFF_FFFF_FFFF_u64],
  ) {
    let weight = f64::from_bits(bits);
    for text in ["alpha", "a"] {
      let mut config = options(1);
      config.initial_similarity = 0.4;
      let mut builder = EntityIndexBuilder::new(config).unwrap();
      let id = builder.intern(0, text).unwrap();
      let mut input = alias(&[id]);
      let form = input.forms.first_mut().unwrap();
      form.minimum_total = weight;
      let token = form.tokens.first_mut().unwrap();
      token.weight = weight;
      token.minimum_weight = weight;
      builder.add_alias(input).unwrap();
      let index = builder.finish(vec![vec![weight]]).unwrap();
      let list = if text == "a" { PostingList::ListedInitial { form: 0, prefix: u16::from(b'a') } } else { PostingList::QueryInitial { form: 0, prefix: 'a' } };
      let entries = decoded(&index, list);
      prop_assert_eq!(entries.len(), 1);
      let impact = entries.first().unwrap().1;
      prop_assert!(impact >= 0.4, "weight={weight}, impact={impact}");
      prop_assert!(impact <= 0.400_01, "weight={weight}, impact={impact}");
    }
  }
}
