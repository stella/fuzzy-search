#![allow(clippy::expect_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use sha2::{Digest, Sha256};
use stella_fuzzy_search_core::entity::lexicon::Lexicon;
use stella_fuzzy_search_core::entity::{
  AliasForm, AliasInput, AliasToken, BudgetBasis, Cost, EditBudget,
  EntityIndexBuilder, EntityIndexOptions, EntityQuery, QueryToken, QueryUnit,
  SearchParams,
};

const ENTITY_COUNT: usize = 20_000;
const QUERY_COUNT: usize = 10_000;

fn name(index: usize) -> [String; 2] {
  let letters = Sha256::digest(format!("synthetic-person-{index}"))
    .iter()
    .take(16)
    .flat_map(|byte| [byte >> 4, byte & 0x0f])
    .map(|nibble| char::from(b'a'.saturating_add(nibble)))
    .collect::<String>();
  [
    letters.chars().take(16).collect(),
    letters.chars().skip(16).collect(),
  ]
}

fn budget() -> EditBudget {
  EditBudget {
    by_length: vec![0, 0, 0, 1, 1, 2],
    basis: BudgetBasis::Shorter,
  }
}

fn query(index: usize) -> EntityQuery {
  let words = name(index);
  EntityQuery {
    tokens: words
      .iter()
      .map(|text| QueryToken {
        form: 0,
        text: text.clone(),
        weight: 1.0,
      })
      .collect(),
    units: words
      .into_iter()
      .enumerate()
      .map(|(position, text)| QueryUnit {
        form: 0,
        text,
        positions: vec![u32::try_from(position).expect("two positions")],
      })
      .collect(),
  }
}

fn bench_search(c: &mut Criterion) {
  let mut builder = EntityIndexBuilder::new(EntityIndexOptions {
    forms: 1,
    budget: budget(),
    initial_weight: 1.0,
    initial_similarity: 0.8,
  })
  .expect("valid options");
  let mut vocabulary = Vec::with_capacity(ENTITY_COUNT.saturating_mul(2));
  for index in 0..ENTITY_COUNT {
    let tokens = name(index)
      .into_iter()
      .map(|text| {
        let id = builder.intern(0, &text).expect("unique vocabulary token");
        vocabulary.push(text);
        AliasToken {
          id,
          weight: 1.0,
          minimum_weight: 0.8,
        }
      })
      .collect();
    builder
      .add_alias(AliasInput {
        entry_id: u32::try_from(index).expect("entity id fits u32"),
        entity_group: 0,
        quality: 1.0,
        forms: vec![AliasForm {
          form: 0,
          tokens,
          joins: vec![],
          minimum_total: 1.6,
        }],
      })
      .expect("valid alias");
  }
  let index = builder
    .finish(vec![vec![1.0; vocabulary.len()]])
    .expect("valid index");
  let lexicon = Lexicon::new(&vocabulary).expect("distinct vocabulary");
  let queries = (0..QUERY_COUNT).map(query).collect::<Vec<_>>();
  for (label, tau) in [("tau_0.8", 0.799_999_999_999_998_3), ("tau_0.9", 0.9)] {
    let mut next = 0;
    c.bench_function(&format!("entity_random_tokens/search/{label}"), |b| {
      b.iter(|| {
        let query = queries.get(next).expect("query index wraps");
        next = next.saturating_add(1) % QUERY_COUNT;
        index
          .search(
            query,
            &SearchParams {
              tau,
              quality: 1.0,
              max_cost: u64::MAX,
            },
          )
          .expect("valid search")
      });
    });
  }
  let mut lookup_index = 0;
  c.bench_function("entity_random_tokens/two_lexicon_lookups", |b| {
    b.iter(|| {
      let query = queries.get(lookup_index).expect("query index wraps");
      lookup_index = lookup_index.saturating_add(1) % QUERY_COUNT;
      let mut cost = Cost::default();
      query
        .tokens
        .iter()
        .map(|token| {
          lexicon
            .lookup(&token.text, &budget(), &mut cost, u64::MAX)
            .expect("valid lookup")
        })
        .collect::<Vec<_>>()
    });
  });
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
