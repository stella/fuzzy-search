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
  assert_eq!(
    decoded(
      &index,
      PostingList::ListedInitial {
        form: 0,
        prefix: high_surrogate
      }
    ),
    [] as [(u32, f64); 0]
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
      left: 2,
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
      &input,
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
  fn caller_mapped_joins_preserve_oracle_completeness(
    weights in prop::collection::vec(1_u16..100, 3),
    cutoff in 0_u16..=1000,
  ) {
    // Original neighbors may map to any pair after a caller interns duplicate
    // positions, including reversed pairs and two occurrences of one position.
    let texts = ["alpha", "beta", "gamma"];
    for left in 0..3_usize {
      for right in 0..3_usize {
        let mut builder = EntityIndexBuilder::new(options(1)).unwrap();
        let ids = texts.iter().map(|text| builder.intern(0, text).unwrap()).collect::<Vec<_>>();
        let joined_text = format!("{}{}", texts.get(left).unwrap(), texts.get(right).unwrap());
        let joined = builder.intern(0, &joined_text).unwrap();
        let mut input = alias(&ids);
        let form = input.forms.first_mut().unwrap();
        for (token, weight) in form.tokens.iter_mut().zip(&weights) {
          token.weight = f64::from(*weight);
          token.minimum_weight = token.weight * 0.5;
        }
        form.minimum_total = form.tokens.iter().map(|token| token.minimum_weight).sum();
        form.joins.push(AliasJoin { id: joined, left: u32::try_from(left).unwrap(), right: u32::try_from(right).unwrap() });
        builder.add_alias(input).unwrap();
        let index = builder.finish(vec![vec![1.0; 4]]).unwrap();
        let params = SearchParams { tau: f64::from(cutoff) / 1000.0, quality: 1.0, max_cost: u64::MAX };
        let joined_query = EntityQuery {
          tokens: vec![QueryToken { form: 0, text: joined_text.clone(), weight: 1.0 }],
          units: vec![QueryUnit { form: 0, text: joined_text, positions: vec![0] }],
        };
        assert_oracle_completeness(&index, &joined_query, &params);
        let mut split_query = EntityQuery {
          tokens: texts.iter().map(|text| QueryToken { form: 0, text: (*text).to_owned(), weight: 1.0 }).collect(),
          units: texts.iter().enumerate().map(|(position, text)| QueryUnit { form: 0, text: (*text).to_owned(), positions: vec![u32::try_from(position).unwrap()] }).collect(),
        };
        // Query units cover a set of positions even when the text repeats one.
        let positions = if left == right { vec![u32::try_from(left).unwrap()] } else { vec![u32::try_from(left).unwrap(), u32::try_from(right).unwrap()] };
        split_query.units.push(QueryUnit { form: 0, text: format!("{}{}", texts.get(left).unwrap(), texts.get(right).unwrap()), positions });
        assert_oracle_completeness(&index, &split_query, &params);
      }
    }
  }
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]
  #[test]
  fn shares_position_weights_and_preserves_independent_form_minima(left in 1_u16..400, right in 401_u16..800) {
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
      if crossed_minimums {
        prop_assert_eq!(builder.add_alias(input.clone()).unwrap(), 1);
        let index = builder.finish(vec![vec![1.0; 2]; 2]).unwrap();
        let restored = EntityIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
        prop_assert_eq!(index.aliases.iter().nth(1).unwrap(), input.clone());
        prop_assert_eq!(restored.aliases.iter().nth(1).unwrap(), input);
      } else {
        prop_assert!(builder.add_alias(input).is_err());
      }
    }
  }

  #[test]
  fn initial_posting_share_is_scale_invariant(
    weight in prop_oneof![Just(1e-150_f64), Just(1e150_f64), 1e-150_f64..=1e150_f64],
  ) {
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

fn one_token_builder() -> EntityIndexBuilder {
  let mut builder = EntityIndexBuilder::new(options(1)).unwrap();
  builder.intern(0, "alpha").unwrap();
  builder
}

#[test]
fn cached_maximum_alias_positions_survives_serialization() {
  for counts in [vec![], vec![1], vec![1, 3, 2]] {
    let mut builder = one_token_builder();
    for &count in &counts {
      builder.add_alias(alias(&vec![0; count])).unwrap();
    }
    let index = builder.finish(vec![vec![1.0]]).unwrap();
    let restored = EntityIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
    let expected = counts.into_iter().max().unwrap_or(0);
    for built in [&index, &restored] {
      assert_eq!(built.maximum_alias_tokens, expected);
    }
  }
}

fn weighted_alias(weight: f64, minimum_weight: f64) -> AliasInput {
  let mut input = alias(&[0]);
  let form = input.forms.first_mut().unwrap();
  form.minimum_total = 1e-150;
  let token = form.tokens.first_mut().unwrap();
  token.weight = weight;
  token.minimum_weight = minimum_weight;
  input
}

#[test]
fn accepts_inclusive_entity_weight_endpoints() {
  for weight in [1e-150, 1e150] {
    let mut config = options(1);
    config.initial_weight = weight;
    assert!(EntityIndexBuilder::new(config).is_ok());
    let mut builder = one_token_builder();
    builder.add_alias(weighted_alias(weight, weight)).unwrap();
    let index = builder.finish(vec![vec![weight]]).unwrap();
    let restored = EntityIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
    assert_eq!(restored.aliases, index.aliases);
    assert_eq!(restored.token_weights, index.token_weights);
    assert_eq!(restored.options, index.options);
  }
}

fn assert_out_of_range_weight(value: f64) {
  let mut config = options(1);
  config.initial_weight = value;
  assert!(matches!(
    EntityIndexBuilder::new(config).unwrap_err().kind(),
    crate::ErrorKind::EntityWeightOutOfRange
  ));
  assert!(matches!(
    one_token_builder()
      .finish(vec![vec![value]])
      .unwrap_err()
      .kind(),
    crate::ErrorKind::EntityWeightOutOfRange
  ));
  let mut builder = one_token_builder();
  assert!(matches!(
    builder
      .add_alias(weighted_alias(value, 1e-150))
      .unwrap_err()
      .kind(),
    crate::ErrorKind::EntityWeightOutOfRange
  ));
  assert!(matches!(
    builder
      .add_alias(weighted_alias(1e150, value))
      .unwrap_err()
      .kind(),
    crate::ErrorKind::EntityWeightOutOfRange
  ));
}

#[test]
fn rejects_just_outside_entity_weight_endpoints_with_typed_error() {
  for weight in [
    1e-150_f64.next_down(),
    1e150_f64.next_up(),
    f64::from_bits(1),
    f64::MAX,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    0.0,
    -1.0,
  ] {
    assert_out_of_range_weight(weight);
  }
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]
  #[test]
  fn rejects_out_of_range_weight_models_with_typed_error(
    weight in prop_oneof![f64::MIN_POSITIVE..1e-150_f64, 1e151_f64..=f64::MAX],
  ) {
    assert_out_of_range_weight(weight);
  }
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(48))]
  #[test]
  fn form_specific_minima_preserve_original_inputs_and_every_posting(
    rows in prop::collection::vec((prop::collection::vec((0_u32..3, 1_u16..1000, any::<u8>()), 3..7), 0_u16..=1000), 1..12),
    form_count in 2_u32..4,
  ) {
    let mut builder = EntityIndexBuilder::new(options(form_count)).unwrap();
    for form in 0..form_count {
      for word in ["alpha", "a", "beta", "alphaalpha", "alphabeta", "日本"] {
        builder.intern(form, word).unwrap();
      }
    }
    let inputs = rows.into_iter().enumerate().map(|(entry_id, (positions, quality))| {
      let forms = (0..form_count).map(|form| {
        let tokens = positions.iter().map(|&(id, weight, discount)| AliasToken {
          id,
          weight: f64::from(weight),
          minimum_weight: f64::from(weight) * (f64::from(discount) + 1.0) / (256.0 * f64::from(form.checked_add(1).unwrap())),
        }).collect::<Vec<_>>();
        let last = u32::try_from(tokens.len().checked_sub(1).unwrap()).unwrap();
        AliasForm {
          form,
          minimum_total: tokens.iter().map(|token| token.minimum_weight).sum(),
          tokens,
          joins: vec![AliasJoin { id: 3, left: 0, right: 0 }, AliasJoin { id: 4, left: last, right: 0 }, AliasJoin { id: 4, left: 0, right: last }],
        }
      }).collect();
      AliasInput { entry_id: u32::try_from(entry_id).unwrap(), entity_group: 0, quality: f64::from(quality) / 1000.0, forms }
    }).collect::<Vec<_>>();
    for input in &inputs { builder.add_alias(input.clone()).unwrap(); }
    let index = builder.finish(vec![vec![1.0; 6]; usize::try_from(form_count).unwrap()]).unwrap();
    let restored = EntityIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
    prop_assert_eq!(&index.aliases.iter().collect::<Vec<_>>(), &inputs);
    prop_assert_eq!(&restored.aliases.iter().collect::<Vec<_>>(), &inputs);
    let original_keys = index.postings.list_keys().into_iter().collect::<std::collections::BTreeSet<_>>();
    let rebuilt_keys = restored.postings.list_keys().into_iter().collect::<std::collections::BTreeSet<_>>();
    prop_assert_eq!(&original_keys, &rebuilt_keys);
    let mut exercised_postings = 0_u64;
    for list in original_keys {
      let original = decoded(&index, list);
      exercised_postings = exercised_postings.checked_add(u64::try_from(original.len()).unwrap()).unwrap();
      prop_assert_eq!(original, decoded(&restored, list));
    }
    prop_assert_eq!(exercised_postings, index.posting_count());
    prop_assert_eq!(exercised_postings, restored.posting_count());
  }
}
