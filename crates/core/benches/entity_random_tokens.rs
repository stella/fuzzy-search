#![allow(
  clippy::arithmetic_side_effects,
  clippy::as_conversions,
  clippy::expect_used,
  clippy::indexing_slicing,
  clippy::shadow_unrelated,
  clippy::too_many_lines
)]

use criterion::{Criterion, criterion_group, criterion_main};
use sha2::{Digest, Sha256};
use stella_fuzzy_search_core::entity::lexicon::Lexicon;
use stella_fuzzy_search_core::entity::{
  AliasForm, AliasInput, AliasJoin, AliasToken, BudgetBasis, Cost, EditBudget,
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

fn six_unit_budget() -> EditBudget {
  EditBudget {
    by_length: vec![0, 0, 0, 0, 1, 1, 2],
    basis: BudgetBasis::Shorter,
  }
}

fn fold(text: &str) -> String {
  text.chars().fold(String::new(), |mut folded, character| {
    if !folded.ends_with(character) {
      folded.push(character);
    }
    folded
  })
}

fn six_unit_query(index: usize) -> EntityQuery {
  let raw = name(index);
  let folded = raw.clone().map(|text| fold(&text));
  let tokens = [
    QueryToken {
      form: 0,
      text: folded[0].clone(),
      weight: 1.0,
    },
    QueryToken {
      form: 0,
      text: folded[1].clone(),
      weight: 1.0,
    },
  ];
  let mut units = Vec::with_capacity(6);
  for (form, words) in [(0, folded), (1, raw)] {
    units.push(QueryUnit {
      form,
      text: words[0].clone(),
      positions: vec![0],
    });
    units.push(QueryUnit {
      form,
      text: words[1].clone(),
      positions: vec![1],
    });
    units.push(QueryUnit {
      form,
      text: words.concat(),
      positions: vec![0, 1],
    });
  }
  EntityQuery {
    tokens: tokens.into(),
    units,
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
  // Built once so the measured lookups time traversal, not budget allocation.
  let lookup_budget = budget();
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
            .lookup(&token.text, &lookup_budget, &mut cost, u64::MAX)
            .expect("valid lookup")
        })
        .collect::<Vec<_>>()
    });
  });

  let mut builder = EntityIndexBuilder::new(EntityIndexOptions {
    forms: 2,
    budget: six_unit_budget(),
    initial_weight: 1.0,
    initial_similarity: 0.8,
  })
  .expect("valid options");
  let mut vocabulary_sizes = [0_usize; 2];
  for entity in 0..ENTITY_COUNT {
    let raw = name(entity);
    let folded = raw.clone().map(|text| fold(&text));
    let mut forms = Vec::with_capacity(2);
    for (form, words) in [(0, folded), (1, raw)] {
      let ids = words
        .clone()
        .map(|text| builder.intern(form, &text).expect("token"));
      let joined = builder.intern(form, &words.concat()).expect("join");
      vocabulary_sizes[form as usize] = vocabulary_sizes[form as usize]
        .max(usize::try_from(joined).expect("id") + 1);
      forms.push(AliasForm {
        form,
        tokens: ids
          .map(|id| AliasToken {
            id,
            weight: 1.0,
            minimum_weight: 0.8,
          })
          .into(),
        joins: vec![AliasJoin {
          id: joined,
          left: 0,
          right: 1,
        }],
        minimum_total: 1.6,
      });
    }
    builder
      .add_alias(AliasInput {
        entry_id: u32::try_from(entity).expect("entity id"),
        entity_group: 0,
        quality: 1.0,
        forms,
      })
      .expect("alias");
  }
  let six_index = builder
    .finish(vocabulary_sizes.map(|size| vec![1.0; size]).into())
    .expect("index");
  let six_queries = (0..QUERY_COUNT).map(six_unit_query).collect::<Vec<_>>();
  let mut next = 0;
  c.bench_function("entity_random_tokens/search/six_units_tau_0.8", |b| {
    b.iter(|| {
      let query = &six_queries[next];
      next = (next + 1) % QUERY_COUNT;
      six_index
        .search(
          query,
          &SearchParams {
            tau: 0.799_999_999_999_998_3,
            quality: 1.0,
            max_cost: u64::MAX,
          },
        )
        .expect("search")
    });
  });
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
