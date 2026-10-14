#![allow(clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use stella_fuzzy_search_core::entity::lexicon::Lexicon;
use stella_fuzzy_search_core::entity::{BudgetBasis, Cost, EditBudget};

const BASE_TOKENS: usize = 256;
const PREFIX: &str = "shared-entity-name-prefix-";

fn token(index: usize) -> String {
  format!("{PREFIX}{index:06}")
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

criterion_group!(benches, bench_lookup);
criterion_main!(benches);
