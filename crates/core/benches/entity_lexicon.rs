#![allow(clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use stella_fuzzy_search_core::entity::lexicon::Lexicon;
use stella_fuzzy_search_core::entity::{BudgetBasis, Cost, EditBudget};

const BASE_TOKENS: usize = 256;
const PREFIX: &str = "shared-entity-name-prefix-";

fn token(index: usize) -> String {
  format!("{PREFIX}{index:06}")
}

// Fixed seed and scalar length make before/after runs directly comparable.
fn random_names() -> Vec<String> {
  let mut state = 0x1234_5678_9abc_def0_u64;
  (0..20_000)
    .map(|_| {
      (0..16)
        .map(|_| {
          state ^= state << 13;
          state ^= state >> 7;
          state ^= state << 17;
          char::from(
            b'a'.saturating_add(u8::try_from(state % 26).expect("letter")),
          )
        })
        .collect()
    })
    .collect()
}

// Explicit benchmark output records deterministic DP work alongside timings.
#[allow(clippy::print_stderr, clippy::disallowed_macros)]
fn bench_random_names(c: &mut Criterion) {
  let tokens = random_names();
  let lexicon = Lexicon::new(&tokens).expect("distinct random names");
  // A screening-equivalent set: one exact, one substitution, one transposition,
  // and one absent name. Each query is measured separately and as a set.
  let first = tokens.first().expect("names");
  let mut substitution = first.clone().into_bytes();
  if let Some(letter) = substitution.get_mut(8) {
    *letter = if *letter == b'a' { b'b' } else { b'a' };
  }
  let mut transposition = first.clone().into_bytes();
  transposition.swap(7, 8);
  let queries = [
    first.clone(),
    String::from_utf8(substitution).expect("ASCII"),
    String::from_utf8(transposition).expect("ASCII"),
    "abcdefghijklmnop".to_owned(),
  ];
  let budget = EditBudget {
    by_length: vec![0, 0, 0, 0, 1, 1, 2],
    basis: BudgetBasis::Shorter,
  };
  let mut group = c.benchmark_group("entity_lexicon_random_20000");
  group.sample_size(10);
  group.measurement_time(std::time::Duration::from_secs(2));
  for (index, query) in queries.iter().enumerate() {
    let mut cost = Cost::default();
    let result = lexicon
      .lookup(query, &budget, &mut cost, u64::MAX)
      .expect("lookup");
    eprintln!(
      "query {index}: {} cells, {} matches",
      cost.lexicon_cells,
      result.matches.len()
    );
    group.bench_with_input(
      BenchmarkId::new("lookup", index),
      query,
      |b, query| {
        b.iter(|| {
          lexicon
            .lookup(
              std::hint::black_box(query),
              &budget,
              &mut Cost::default(),
              u64::MAX,
            )
            .expect("lookup")
        });
      },
    );
  }
  group.bench_function("screening_set", |b| {
    b.iter(|| {
      for query in &queries {
        std::hint::black_box(
          lexicon
            .lookup(
              std::hint::black_box(query),
              &budget,
              &mut Cost::default(),
              u64::MAX,
            )
            .expect("lookup"),
        );
      }
    });
  });
  group.finish();
}

fn bench_lookup(c: &mut Criterion) {
  let mut group = c.benchmark_group("entity_lexicon_osa_lookup");
  let budget = EditBudget {
    by_length: vec![0, 0, 0, 0, 1],
    basis: BudgetBasis::Shorter,
  };

  for multiplier in [1, 2, 5] {
    let tokens = (0..BASE_TOKENS.saturating_mul(multiplier))
      .map(token)
      .collect::<Vec<_>>();
    let lexicon = Lexicon::new(&tokens).expect("distinct benchmark tokens");
    let query = format!(
      "{}x",
      token(BASE_TOKENS.saturating_mul(multiplier).div_euclid(2))
    );
    group.bench_with_input(
      BenchmarkId::new("shared_prefix_tokens", tokens.len()),
      &query,
      |b, query| {
        b.iter(|| {
          let mut cost = Cost::default();
          lexicon
            .lookup(query, &budget, &mut cost, u64::MAX)
            .expect("valid benchmark lookup")
        });
      },
    );
  }
  group.finish();
}

criterion_group!(benches, bench_lookup, bench_random_names);
criterion_main!(benches);
