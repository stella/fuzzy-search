//! Compact vocabulary and complete, deterministically ordered OSA lookup.

use std::collections::{BTreeSet, HashMap};
use std::mem::size_of;

use fst::{Map, MapBuilder};

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
  gram_postings: HashMap<(char, char), Vec<TokenId>>,
  lexical_order: Vec<usize>,
}

fn fst_error(error: &fst::Error) -> Error {
  Error::from_reason(format!("Invalid entity lexicon: {error}"))
}

fn float_count(value: usize) -> Result<f64> {
  u32::try_from(value)
    .map(f64::from)
    .map_err(|_| Error::from_reason("Entity count exceeds u32"))
}

/// Bytes allocated by a std `HashMap` table holding `capacity` usable entries
/// of `entry` bytes. The table keeps a power-of-two bucket count at a 7/8
/// load factor (4 or 8 buckets below eight entries), one control byte per
/// bucket and one trailing control group.
fn hash_table_bytes(capacity: usize, entry: usize) -> usize {
  const CONTROL_GROUP: usize = 16;
  if capacity == 0 {
    return 0;
  }
  let buckets = if capacity < 4 {
    4
  } else if capacity < 8 {
    8
  } else {
    capacity.saturating_mul(8).div_ceil(7).next_power_of_two()
  };
  buckets
    .saturating_mul(entry.saturating_add(1))
    .saturating_add(CONTROL_GROUP)
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
    let lexical_order = ordered.iter().map(|(id, _)| *id).collect();
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
      gram_postings: HashMap::new(),
      lexical_order,
    };
    u32::try_from(tokens.len())
      .map_err(|_| Error::from_reason("Entity vocabulary exceeds u32"))?;
    for (id, text) in tokens.iter().enumerate() {
      lexicon
        .lengths
        .push(u16::try_from(text.chars().count()).map_err(|_| {
          Error::from_reason("Entity token exceeds u16 length")
        })?);
      let grams = bigrams(text);
      let id = u32::try_from(id)
        .map_err(|_| Error::from_reason("Entity token id exceeds u32"))?;
      for gram in &grams {
        lexicon.gram_postings.entry(*gram).or_default().push(id);
      }
      lexicon
        .gram_counts
        .push(u32::try_from(grams.len()).map_err(|_| {
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
    for postings in lexicon.gram_postings.values_mut() {
      postings.shrink_to_fit();
    }
    lexicon.gram_postings.shrink_to_fit();
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
    let postings_bytes =
      self.gram_postings.values().fold(0_usize, |bytes, ids| {
        bytes
          .saturating_add(ids.capacity().saturating_mul(size_of::<TokenId>()))
      });
    size_of::<Self>()
      .saturating_add(postings_bytes)
      .saturating_add(
        self
          .lexical_order
          .capacity()
          .saturating_mul(size_of::<usize>()),
      )
      .saturating_add(hash_table_bytes(
        self.gram_postings.capacity(),
        size_of::<((char, char), Vec<TokenId>)>(),
      ))
      .saturating_add(self.map.as_fst().as_inner().capacity())
      .saturating_add(self.text.capacity())
      .saturating_add(self.offsets.capacity().saturating_mul(size_of::<u32>()))
      .saturating_add(self.lengths.capacity().saturating_mul(size_of::<u16>()))
      .saturating_add(
        self.gram_counts.capacity().saturating_mul(size_of::<u32>()),
      )
  }

  fn shared_bigram_counts(
    &self,
    grams: &BTreeSet<(char, char)>,
  ) -> Result<Vec<u32>> {
    let mut shared_counts = vec![0_u32; self.len()];
    for gram in grams {
      if let Some(postings) = self.gram_postings.get(gram) {
        for &id in postings {
          let shared = shared_counts
            .get_mut(
              usize::try_from(id)
                .map_err(|_| Error::from_reason("Invalid entity token id"))?,
            )
            .ok_or_else(|| Error::from_reason("Missing entity bigram count"))?;
          *shared = shared.saturating_add(1);
        }
      }
    }
    Ok(shared_counts)
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
    let mut remaining = max_cost.saturating_sub(cost.total());
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
    let shared_counts = self.shared_bigram_counts(&grams)?;
    let automaton = OsaAutomaton::new(query, maximum_distance, remaining);
    let mut fuzzy = Vec::new();
    for &index in &self.lexical_order {
      let (&length, (&gram_count, &shared)) = self
        .lengths
        .get(index)
        .zip(self.gram_counts.get(index).zip(shared_counts.get(index)))
        .ok_or_else(|| Error::from_reason("Missing entity lexicon metadata"))?;
      let id = u32::try_from(index)
        .map_err(|_| Error::from_reason("Invalid entity token id"))?;
      if exact == Some(u64::from(id)) {
        continue;
      }
      let length = usize::from(length);
      let allowed = usize::from(budget.for_length(query_length.min(length)));
      if query_length.abs_diff(length) > allowed {
        continue;
      }
      // An insertion/deletion/substitution destroys at most two padded bigrams
      // on either side; an adjacent transposition destroys at most three:
      // xaby -> xbay replaces xa, ab, by with xb, ba, ay. Along any OSA edit
      // path each directional set difference is therefore <= 3 * distance.
      // Repeated grams and literal padding characters can only weaken the bound.
      let candidate_count = usize::try_from(gram_count)
        .map_err(|_| Error::from_reason("Invalid entity bigram count"))?;
      let shared = usize::try_from(shared)
        .map_err(|_| Error::from_reason("Invalid entity bigram count"))?;
      if grams.len().max(candidate_count).saturating_sub(shared)
        > allowed.saturating_mul(3)
      {
        continue;
      }
      let token = self
        .token(id)
        .ok_or_else(|| Error::from_reason("Missing entity token"))?;
      let Some(distance) = automaton.distance_for(token) else {
        if automaton.exhausted() {
          break;
        }
        continue;
      };
      if distance > allowed {
        continue;
      }
      if !automaton.charge_match() {
        break;
      }
      let distance = u8::try_from(distance)
        .map_err(|_| Error::from_reason("Invalid entity distance"))?;
      let similarity =
        1.0 - f64::from(distance) / float_count(query_length.max(length))?;
      let bigram_similarity =
        float_count(shared)? / float_count(grams.len().max(candidate_count))?;
      fuzzy.push(LexiconMatch {
        id,
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

  #[test]
  fn hash_table_bytes_counts_allocated_buckets() {
    assert_eq!(hash_table_bytes(0, 24), 0);
    assert_eq!(hash_table_bytes(3, 24), 4 * 25 + 16);
    assert_eq!(hash_table_bytes(7, 24), 8 * 25 + 16);
    // 14 usable entries live in 16 buckets, 28 in 32.
    assert_eq!(hash_table_bytes(14, 24), 16 * 25 + 16);
    assert_eq!(hash_table_bytes(28, 24), 32 * 25 + 16);
    let mut map: HashMap<(char, char), Vec<TokenId>> = HashMap::new();
    map.insert(('a', 'b'), Vec::new());
    let capacity = map.capacity();
    assert!(
      hash_table_bytes(capacity, size_of::<((char, char), Vec<TokenId>)>())
        >= capacity.saturating_mul(size_of::<((char, char), Vec<TokenId>)>())
    );
  }

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
        'a', 'b', 'c', 'é', '\u{301}', '日', '本', 'ع', 'ر', '𐐀', '😀', '^',
        '$',
      ]),
      0..12,
    )
    .prop_map(|chars| chars.into_iter().collect())
  }

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn prefilters_preserve_edited_queries_at_every_budget(
      query in word(),
      vocabulary in prop::collection::btree_set(word(), 0..20),
      edits in prop::collection::vec((0_usize..16, any::<bool>()), 0..5),
      mut table in prop::collection::vec(0_u8..=12, 1..16),
    ) {
      let mut vocabulary = vocabulary;
      let mut chars = query.chars().collect::<Vec<_>>();
      vocabulary.insert(query.clone());
      for (position, transpose) in edits {
        let index = position.checked_rem(chars.len().max(1)).unwrap_or(0);
        if transpose && index.saturating_add(1) < chars.len() {
          chars.swap(index, index.saturating_add(1));
        } else {
          chars.insert(index, '𐐀');
        }
        vocabulary.insert(chars.iter().collect());
      }
      // OSA must not be replaced by unrestricted Damerau when edits intervene.
      vocabulary.extend(["ca".to_owned(), "abc".to_owned()]);
      let tokens = vocabulary.into_iter().collect::<Vec<_>>();
      let lexicon = Lexicon::new(&tokens).unwrap();
      table.sort_unstable();
      let maximum = tokens.iter().map(|text| text.chars().count())
        .chain(std::iter::once(query.chars().count())).max().unwrap();
      let tables = (0..=u8::try_from(maximum).unwrap())
        .chain(std::iter::once(u8::MAX)).map(|limit| vec![limit])
        .chain(std::iter::once(table));
      for by_length in tables {
        let budget = EditBudget { by_length, basis: BudgetBasis::Shorter };
        let result = lexicon.lookup(&query, &budget, &mut Cost::default(), u64::MAX).unwrap();
        prop_assert!(!result.exhausted);
        prop_assert_eq!(result.matches, oracle(&tokens, &query, &budget));
      }
    }

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
      for maximum in 0..=4 {
        let budget = EditBudget {
          by_length: vec![maximum],
          basis: BudgetBasis::Shorter,
        };
        let result = lexicon
          .lookup(query, &budget, &mut Cost::default(), u64::MAX)
          .unwrap();
        assert_eq!(result.matches, oracle(&tokens, query, &budget));
      }
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
