//! Compact vocabulary and complete, deterministically ordered OSA lookup.

use std::collections::BTreeSet;
use std::mem::size_of;

use fst::{IntoStreamer, Map, MapBuilder, Streamer};

use super::osa_walk::OsaAutomaton;
use super::{BudgetBasis, Cost, EditBudget, TokenId};
use crate::{Error, Result};

/// One vocabulary match. Similarity is based on edit distance; bigram
/// similarity controls enumeration order only.
#[derive(Debug, Clone, PartialEq)]
pub struct LexiconMatch {
  pub id: TokenId,
  pub distance: u8,
  pub similarity: f64,
  pub bigram_similarity: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexiconSearch {
  pub matches: Vec<LexiconMatch>,
  pub exhausted: bool,
}

/// UTF-8 FST with caller-order ids and a compact reverse token table.
#[derive(Debug, Clone)]
pub struct Lexicon {
  map: Map<Vec<u8>>,
  text: Vec<u8>,
  offsets: Vec<u32>,
  lengths: Vec<u16>,
  gram_counts: Vec<u32>,
}

fn fst_error(error: &fst::Error) -> Error {
  Error::from_reason(format!("Invalid entity lexicon: {error}"))
}

fn float_count(value: usize) -> Result<f64> {
  u32::try_from(value)
    .map(f64::from)
    .map_err(|_| Error::from_reason("Entity count exceeds u32"))
}

fn bigrams(text: &str) -> BTreeSet<(char, char)> {
  let mut previous = '^';
  let mut grams = BTreeSet::new();
  for character in text.chars().chain(std::iter::once('$')) {
    grams.insert((previous, character));
    previous = character;
  }
  grams
}

impl EditBudget {
  /// Reject non-monotone tables: the shorter-length rule relies on monotonicity.
  pub fn validate(&self) -> Result<()> {
    if self.by_length.is_empty()
      || self
        .by_length
        .windows(2)
        .any(|pair| matches!(pair, [left, right] if left > right))
    {
      return Err(Error::from_reason(
        "Edit budgets must be nonempty and monotone",
      ));
    }
    match self.basis {
      BudgetBasis::Shorter => Ok(()),
    }
  }

  #[must_use]
  pub fn for_length(&self, length: usize) -> u8 {
    self
      .by_length
      .get(length)
      .or_else(|| self.by_length.last())
      .copied()
      .unwrap_or(0)
  }
}

impl Lexicon {
  /// Build a distinct vocabulary. Ids follow `tokens`, never FST sort order.
  pub fn new(tokens: &[String]) -> Result<Self> {
    let mut ordered = tokens.iter().enumerate().collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|(_, text)| *text);
    if ordered
      .windows(2)
      .any(|pair| matches!(pair, [(_, left), (_, right)] if left == right))
    {
      return Err(Error::from_reason(
        "Entity vocabulary tokens must be distinct",
      ));
    }
    let mut builder = MapBuilder::memory();
    for (id, text) in ordered {
      builder
        .insert(
          text,
          u64::try_from(id)
            .map_err(|_| Error::from_reason("Entity token id exceeds u64"))?,
        )
        .map_err(|error| fst_error(&error))?;
    }
    let mut lexicon = Self {
      map: Map::new(builder.into_inner().map_err(|error| fst_error(&error))?)
        .map_err(|error| fst_error(&error))?,
      text: Vec::new(),
      offsets: vec![0],
      lengths: Vec::with_capacity(tokens.len()),
      gram_counts: Vec::with_capacity(tokens.len()),
    };
    u32::try_from(tokens.len())
      .map_err(|_| Error::from_reason("Entity vocabulary exceeds u32"))?;
    for text in tokens {
      lexicon
        .lengths
        .push(u16::try_from(text.chars().count()).map_err(|_| {
          Error::from_reason("Entity token exceeds u16 length")
        })?);
      lexicon
        .gram_counts
        .push(u32::try_from(bigrams(text).len()).map_err(|_| {
          Error::from_reason("Entity bigram count exceeds u32")
        })?);
      let end = lexicon
        .text
        .len()
        .checked_add(text.len())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
          Error::from_reason("Entity vocabulary text exceeds u32")
        })?;
      lexicon.text.extend_from_slice(text.as_bytes());
      lexicon.offsets.push(end);
    }
    lexicon.text.shrink_to_fit();
    lexicon.offsets.shrink_to_fit();
    Ok(lexicon)
  }

  #[must_use]
  pub const fn len(&self) -> usize {
    self.lengths.len()
  }

  #[must_use]
  pub const fn is_empty(&self) -> bool {
    self.lengths.is_empty()
  }

  #[must_use]
  pub fn token(&self, id: TokenId) -> Option<&str> {
    let index = usize::try_from(id).ok()?;
    let start = usize::try_from(*self.offsets.get(index)?).ok()?;
    let end =
      usize::try_from(*self.offsets.get(index.checked_add(1)?)?).ok()?;
    std::str::from_utf8(self.text.get(start..end)?).ok()
  }

  #[must_use]
  pub fn char_len(&self, id: TokenId) -> Option<u16> {
    self.lengths.get(usize::try_from(id).ok()?).copied()
  }

  /// Includes owned buffers and the lexicon's inline storage, without allocator overhead.
  #[must_use]
  pub fn native_bytes(&self) -> usize {
    size_of::<Self>()
      .saturating_add(self.map.as_fst().as_inner().capacity())
      .saturating_add(self.text.capacity())
      .saturating_add(self.offsets.capacity().saturating_mul(size_of::<u32>()))
      .saturating_add(self.lengths.capacity().saturating_mul(size_of::<u16>()))
      .saturating_add(
        self.gram_counts.capacity().saturating_mul(size_of::<u32>()),
      )
  }

  /// Lookup charges every evaluated DP cell and every returned match.
  /// Incomplete results explicitly report exhaustion and never exceed `max_cost`.
  pub fn lookup(
    &self,
    query: &str,
    budget: &EditBudget,
    cost: &mut Cost,
    max_cost: u64,
  ) -> Result<LexiconSearch> {
    budget.validate()?;
    let spent = cost.total();
    let mut remaining = max_cost.saturating_sub(spent);
    let mut matches = Vec::new();
    let exact = self.map.get(query);
    if let Some(id) = exact {
      if remaining == 0 {
        return Ok(LexiconSearch {
          matches,
          exhausted: true,
        });
      }
      remaining = remaining.saturating_sub(1);
      cost.lexicon_matches = cost.lexicon_matches.saturating_add(1);
      matches.push(LexiconMatch {
        id: u32::try_from(id)
          .map_err(|_| Error::from_reason("Invalid entity token id"))?,
        distance: 0,
        similarity: 1.0,
        bigram_similarity: 1.0,
      });
    }
    let query_length = query.chars().count();
    let maximum_distance = budget.for_length(query_length);
    if maximum_distance == 0 || self.is_empty() {
      return Ok(LexiconSearch {
        matches,
        exhausted: false,
      });
    }
    let grams = bigrams(query);
    let automaton = OsaAutomaton::new(query, maximum_distance, remaining);
    let mut stream = self.map.search_with_state(&automaton).into_stream();
    let mut fuzzy = Vec::new();
    while let Some((key, id, state)) = stream.next() {
      if Some(id) == exact {
        continue;
      }
      let distance = state.distance(
        query_length,
        usize::from(maximum_distance).saturating_add(1),
      );
      if distance
        > usize::from(budget.for_length(query_length.min(state.length)))
      {
        continue;
      }
      if !automaton.charge_match() {
        break;
      }
      let token = std::str::from_utf8(key)
        .map_err(|_| Error::from_reason("Invalid UTF-8 in entity lexicon"))?;
      let candidate_grams = bigrams(token);
      let shared = grams.intersection(&candidate_grams).count();
      let distance = u8::try_from(distance)
        .map_err(|_| Error::from_reason("Invalid entity distance"))?;
      let similarity = 1.0
        - f64::from(distance) / float_count(query_length.max(state.length))?;
      let bigram_similarity = float_count(shared)?
        / float_count(grams.len().max(candidate_grams.len()))?;
      fuzzy.push(LexiconMatch {
        id: u32::try_from(id)
          .map_err(|_| Error::from_reason("Invalid entity token id"))?,
        distance,
        similarity,
        bigram_similarity,
      });
    }
    fuzzy.sort_unstable_by(|left, right| {
      right
        .bigram_similarity
        .total_cmp(&left.bigram_similarity)
        .then_with(|| left.id.cmp(&right.id))
    });
    matches.extend(fuzzy);
    cost.lexicon_cells = cost.lexicon_cells.saturating_add(automaton.cells());
    cost.lexicon_matches =
      cost.lexicon_matches.saturating_add(automaton.matches());
    Ok(LexiconSearch {
      matches,
      exhausted: automaton.exhausted(),
    })
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
  use super::*;
  use crate::damerau_levenshtein;
  use proptest::prelude::*;

  fn default_budget() -> EditBudget {
    EditBudget {
      by_length: vec![0, 0, 0, 0, 1, 1, 2],
      basis: BudgetBasis::Shorter,
    }
  }

  fn oracle(
    tokens: &[String],
    query: &str,
    budget: &EditBudget,
  ) -> Vec<LexiconMatch> {
    let query_chars = query.chars().collect::<Vec<_>>();
    let grams = bigrams(query);
    let mut matches = tokens
      .iter()
      .enumerate()
      .filter_map(|(id, text)| {
        let chars = text.chars().collect::<Vec<_>>();
        let distance = damerau_levenshtein(&query_chars, &chars);
        if distance
          > usize::from(budget.for_length(chars.len().min(query_chars.len())))
        {
          return None;
        }
        let candidate_grams = bigrams(text);
        let similarity = if distance == 0 {
          1.0
        } else {
          1.0
            - float_count(distance).unwrap()
              / float_count(query_chars.len().max(chars.len())).unwrap()
        };
        let bigram_similarity =
          float_count(grams.intersection(&candidate_grams).count()).unwrap()
            / float_count(grams.len().max(candidate_grams.len())).unwrap();
        Some(LexiconMatch {
          id: u32::try_from(id).unwrap(),
          distance: u8::try_from(distance).unwrap(),
          similarity,
          bigram_similarity,
        })
      })
      .collect::<Vec<_>>();
    matches.sort_unstable_by(|left, right| {
      (right.distance == 0)
        .cmp(&(left.distance == 0))
        .then_with(|| {
          right.bigram_similarity.total_cmp(&left.bigram_similarity)
        })
        .then_with(|| left.id.cmp(&right.id))
    });
    matches
  }

  fn word() -> impl Strategy<Value = String> {
    prop::collection::vec(
      prop::sample::select(vec![
        'a', 'b', 'c', 'é', '日', '本', 'ع', 'ر', '𐐀', '😀',
      ]),
      0..12,
    )
    .prop_map(|chars| chars.into_iter().collect())
  }

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn lookup_equals_full_osa_oracle(
      tokens in prop::collection::btree_set(word(), 0..35),
      query in word(),
      maximum in 0_u8..=4,
      cap in 0_u64..500,
    ) {
      let mut tokens = tokens.into_iter().collect::<Vec<_>>();
      // Deliberately make caller order differ from FST byte order.
      tokens.reverse();
      let lexicon = Lexicon::new(&tokens).unwrap();
      let budget = EditBudget {
        by_length: (0_u8..=12).map(|length| length.min(maximum)).collect(),
        basis: BudgetBasis::Shorter,
      };
      let mut full_cost = Cost::default();
      let full = lexicon.lookup(&query, &budget, &mut full_cost, u64::MAX).unwrap();
      prop_assert!(!full.exhausted);
      prop_assert_eq!(&full.matches, &oracle(&tokens, &query, &budget));
      let mut repeated_cost = Cost::default();
      let repeated = lexicon.lookup(&query, &budget, &mut repeated_cost, u64::MAX).unwrap();
      prop_assert_eq!(full, repeated);
      prop_assert_eq!(full_cost, repeated_cost);
      let mut capped_cost = Cost::default();
      let capped = lexicon.lookup(&query, &budget, &mut capped_cost, cap).unwrap();
      prop_assert!(capped_cost.lexicon_cells.saturating_add(capped_cost.lexicon_matches) <= cap);
      if !capped.exhausted {
        prop_assert_eq!(&capped.matches, &oracle(&tokens, &query, &budget));
      }
      for (id, text) in tokens.iter().enumerate() {
        prop_assert_eq!(lexicon.token(u32::try_from(id).unwrap()), Some(text.as_str()));
      }
    }
  }

  #[test]
  fn boundary_budgets_transpositions_and_non_bmp_match_distance() {
    let tokens = [
      "abcd",
      "abdc",
      "abc",
      "abcde",
      "日𐐀ع本",
      "𐐀日ع本",
      "aaaa",
      "aaab",
      "CA",
      "ABC",
    ]
    .map(str::to_owned);
    let lexicon = Lexicon::new(&tokens).unwrap();
    for query in &tokens {
      let mut cost = Cost::default();
      let result = lexicon
        .lookup(query, &default_budget(), &mut cost, u64::MAX)
        .unwrap();
      assert_eq!(result.matches, oracle(&tokens, query, &default_budget()));
    }
    // Restricted Damerau (OSA) differs from unrestricted Damerau here.
    assert_eq!(damerau_levenshtein(&['C', 'A'], &['A', 'B', 'C']), 3);
  }

  #[test]
  fn exhaustive_small_alphabet_matches_the_full_matrix() {
    let mut tokens = vec![String::new()];
    for _ in 0..6 {
      let previous = tokens.clone();
      for prefix in previous {
        for character in ['a', 'b'] {
          let text = format!("{prefix}{character}");
          if !tokens.contains(&text) {
            tokens.push(text);
          }
        }
      }
    }
    tokens.reverse();
    let lexicon = Lexicon::new(&tokens).unwrap();
    for maximum in 0..=3 {
      let budget = EditBudget {
        by_length: vec![maximum],
        basis: BudgetBasis::Shorter,
      };
      for query in &tokens {
        let result = lexicon
          .lookup(query, &budget, &mut Cost::default(), u64::MAX)
          .unwrap();
        assert_eq!(
          result.matches,
          oracle(&tokens, query, &budget),
          "query {query:?}, budget {maximum}"
        );
      }
    }
  }

  #[test]
  fn validates_vocabulary_and_monotone_budget() {
    assert!(Lexicon::new(&["a".to_owned(), "a".to_owned()]).is_err());
    assert!(
      EditBudget {
        by_length: vec![],
        basis: BudgetBasis::Shorter
      }
      .validate()
      .is_err()
    );
    assert!(
      EditBudget {
        by_length: vec![0, 2, 1],
        basis: BudgetBasis::Shorter
      }
      .validate()
      .is_err()
    );
    let token = "a".repeat(usize::from(u16::MAX).saturating_add(1));
    assert!(Lexicon::new(&[token]).is_err());
  }
}
