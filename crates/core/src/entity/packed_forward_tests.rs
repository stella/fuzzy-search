//! Input reconstruction and sequence sharing, independent of serialization.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::super::{AliasForm, AliasInput, AliasJoin, AliasToken};
use super::ForwardStore;

fn assert_storage(inputs: &[AliasInput]) {
  let weights = inputs
    .iter()
    .map(|alias| {
      alias
        .forms
        .first()
        .unwrap()
        .tokens
        .iter()
        .map(|token| (token.weight.to_bits(), token.minimum_weight.to_bits()))
        .collect::<Vec<_>>()
    })
    .collect::<BTreeSet<_>>();
  let positions = inputs
    .iter()
    .flat_map(|alias| &alias.forms)
    .map(|form| {
      form
        .joins
        .iter()
        .map(|join| (join.left, join.right))
        .collect::<Vec<_>>()
    })
    .collect::<BTreeSet<_>>();
  let store = ForwardStore::new(inputs.to_vec()).unwrap();
  assert_eq!(store.iter().collect::<Vec<_>>(), inputs);
  assert_eq!(store.len(), inputs.len());
  assert_eq!(
    store.weights.len(),
    weights.iter().map(Vec::len).sum::<usize>(),
    "each distinct weight sequence must be stored once across aliases and forms"
  );
  assert_eq!(
    store.positions.len(),
    positions.iter().map(Vec::len).sum::<usize>(),
    "each distinct join-position sequence must be stored once"
  );
  for (id, input) in inputs.iter().enumerate() {
    assert_eq!(store.get(id).unwrap().to_input(), *input);
  }
  assert!(store.get(inputs.len()).is_none());
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]
  #[test]
  fn reconstructs_original_inputs_and_interns_shared_sequences(
    patterns in prop::collection::vec(prop::collection::vec((1_u16..1000, any::<u8>()), 3..7), 1..5),
    rows in prop::collection::vec((any::<u8>(), any::<u32>(), 0_u16..=1000, any::<u8>()), 1..20),
    form_count in 1_u32..4,
  ) {
    let inputs = rows.into_iter().enumerate().map(|(entry, (pattern, seed, quality, join_mask))| {
      let weights = patterns.get(usize::from(pattern).checked_rem(patterns.len()).unwrap()).unwrap();
      let forms = (0..form_count).map(|form| {
        let tokens = weights.iter().enumerate().map(|(position, &(weight, discount))| AliasToken {
          id: seed.wrapping_add(form.wrapping_mul(31)).wrapping_add(u32::try_from(position % 2).unwrap()),
          weight: f64::from(weight),
          minimum_weight: f64::from(weight) * (f64::from(discount) + 1.0) / 256.0,
        }).collect::<Vec<_>>();
        let last = u32::try_from(tokens.len().checked_sub(1).unwrap()).unwrap();
        let joins = [(0, 0), (last, 0), (0, last)].into_iter().enumerate().filter_map(|(position, (left, right))| {
          if join_mask & (1 << position) == 0 { return None; }
          Some(AliasJoin { id: seed.wrapping_add(form).wrapping_add(u32::try_from(position).unwrap()), left, right })
        }).collect();
        AliasForm {
          form,
          minimum_total: tokens.iter().map(|token| token.minimum_weight).sum(),
          tokens,
          joins,
        }
      }).collect();
      AliasInput {
        entry_id: u32::try_from(entry).unwrap(),
        entity_group: seed,
        quality: f64::from(quality) / 1000.0,
        forms,
      }
    }).collect::<Vec<_>>();
    assert_storage(&inputs);
  }
}

#[test]
fn empty_store_has_no_reconstructed_aliases_or_shared_sequences() {
  assert_storage(&[]);
}
