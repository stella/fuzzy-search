#![no_main]

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use stella_fuzzy_search_core::entity::lexicon::Lexicon;
use stella_fuzzy_search_core::entity::{BudgetBasis, Cost, EditBudget};
use stella_fuzzy_search_core::{distance, Metric};

const MAX_WORDS: usize = 24;
const MAX_CHARS: usize = 32;

fn bounded(text: &str) -> String {
  text.chars().take(MAX_CHARS).collect()
}

fuzz_target!(|data: &[u8]| {
  let input = String::from_utf8_lossy(data);
  let mut lines = input.lines();
  let query = bounded(lines.next().unwrap_or_default());
  let mut tokens = Vec::new();
  for line in lines.take(MAX_WORDS) {
    let word = bounded(line);
    if !tokens.contains(&word) {
      tokens.push(word);
    }
  }

  let Ok(lexicon) = Lexicon::new(&tokens) else {
    return;
  };
  let budget = EditBudget {
    by_length: vec![2; MAX_CHARS + 1],
    basis: BudgetBasis::Shorter,
  };
  let mut cost = Cost::default();
  let result = lexicon
    .lookup(&query, &budget, &mut cost, u64::MAX)
    .expect("bounded valid lexicon and budget");
  assert!(!result.exhausted);

  let query_len = query.chars().count();
  let expected_distances = tokens
    .iter()
    .enumerate()
    .filter_map(|(id, token)| {
      let token_len = token.chars().count();
      let allowed = usize::from(budget.for_length(query_len.min(token_len)));
      let distance = distance(&query, token, Metric::DamerauLevenshtein);
      (distance <= u32::try_from(allowed).expect("small bound")).then(|| {
        (
          u32::try_from(id).expect("bounded token count"),
          u8::try_from(distance).expect("small distance"),
        )
      })
    })
    .collect::<BTreeMap<_, _>>();
  let actual_distances = result
    .matches
    .iter()
    .map(|matched| (matched.id, matched.distance))
    .collect::<BTreeMap<_, _>>();
  assert_eq!(actual_distances, expected_distances);

  let mut saw_fuzzy = false;
  let mut previous_bigram = f64::INFINITY;
  let mut previous_id = 0;
  for matched in &result.matches {
    if matched.distance == 0 {
      assert!(!saw_fuzzy, "exact result follows a fuzzy result");
    } else {
      saw_fuzzy = true;
      assert!(matched.bigram_similarity <= previous_bigram);
      if matched.bigram_similarity == previous_bigram {
        assert!(matched.id >= previous_id);
      }
      previous_bigram = matched.bigram_similarity;
      previous_id = matched.id;
    }
  }
});
