//! Indexed entity-name candidate retrieval.
//!
//! Spellings and weights are supplied by the caller; the index does not
//! normalize text or compute logarithmic weights.

mod builder;
mod cost;
mod forward;
pub mod lexicon;
#[cfg(test)]
mod oracle;
mod osa_walk;
mod postings;
pub mod search;
mod serialize;
#[cfg(test)]
mod storage_tests;

pub use builder::EntityIndexBuilder;

use std::collections::BTreeSet;
use std::mem::size_of;
use std::sync::Arc;

use lexicon::Lexicon;
use postings::Postings;
use search::{MAX_WEIGHT, MIN_WEIGHT, PostingList, SearchIndex};

use crate::Result;

fn validate_weight(weight: f64) -> Result<()> {
  if !weight.is_finite() || !(MIN_WEIGHT..=MAX_WEIGHT).contains(&weight) {
    return Err(crate::Error {
      reason: "Entity weights and minimum weights must be finite and within [1e-150, 1e150]".into(),
      kind: crate::ErrorKind::EntityWeightOutOfRange,
    });
  }
  Ok(())
}

/// A spelling-form index, local to an index instance.
pub type FormId = u32;
/// A token identifier in caller insertion order, local to a spelling form.
pub type TokenId = u32;
/// An alias identifier in builder insertion order.
pub type AliasId = u32;

/// Which token length controls the permitted edit distance.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BudgetBasis {
  Shorter,
}

/// Monotone edit budgets indexed by Unicode scalar length.
/// Lengths beyond the table use its last element.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EditBudget {
  pub by_length: Vec<u8>,
  pub basis: BudgetBasis,
}

/// Construction options shared by every spelling form.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityIndexOptions {
  pub forms: u32,
  pub budget: EditBudget,
  pub initial_weight: f64,
  pub initial_similarity: f64,
}

/// One alias token position, including caller-computed weight bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct AliasToken {
  pub id: TokenId,
  pub weight: f64,
  pub minimum_weight: f64,
}

/// A caller-declared joined pair, interned in the same vocabulary.
/// Positions may repeat or be reordered after caller-side token deduplication.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AliasJoin {
  pub id: TokenId,
  pub left: u32,
  pub right: u32,
}

/// One spelling form of an alias. All forms share position weights and minima.
#[derive(Debug, Clone, PartialEq)]
pub struct AliasForm {
  pub form: FormId,
  pub tokens: Vec<AliasToken>,
  pub joins: Vec<AliasJoin>,
  pub minimum_total: f64,
}

/// Caller-owned identity and forward data for one alias.
#[derive(Debug, Clone, PartialEq)]
pub struct AliasInput {
  pub entry_id: u32,
  pub entity_group: u32,
  pub quality: f64,
  pub forms: Vec<AliasForm>,
}

/// A query token position. Text is already normalized for its spelling form.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryToken {
  pub form: FormId,
  pub text: String,
  pub weight: f64,
}

/// A query lookup unit, possibly joining multiple token positions.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct QueryUnit {
  pub form: FormId,
  pub text: String,
  pub positions: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntityQuery {
  pub tokens: Vec<QueryToken>,
  pub units: Vec<QueryUnit>,
}

/// Search threshold and deterministic work ceiling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchParams {
  pub tau: f64,
  pub quality: f64,
  pub max_cost: u64,
}

/// A vocabulary hit for a query unit.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitMatch {
  pub unit: u32,
  pub form: FormId,
  pub id: TokenId,
  pub similarity: f64,
}

/// Deterministic charged work, independent of wall-clock time.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct Cost {
  pub lexicon_cells: u64,
  pub lexicon_matches: u64,
  pub sorted_accesses: u64,
  pub random_access_pairs: u64,
  pub survivors: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
  pub unit_matches: Vec<UnitMatch>,
  pub query_weights: Vec<f64>,
  pub survivors: Vec<AliasId>,
  pub cost: Cost,
  pub exhausted: bool,
}

/// Immutable index, shareable between searches and serialization.
#[derive(Debug)]
pub struct EntityIndex {
  maximum_alias_tokens: usize,
  options: EntityIndexOptions,
  lexicons: Vec<Arc<Lexicon>>,
  aliases: Vec<AliasInput>,
  postings: Postings,
  token_weights: Vec<Vec<f64>>,
}

impl EntityIndex {
  pub fn search(
    &self,
    query: &EntityQuery,
    params: &SearchParams,
  ) -> Result<SearchResult> {
    search::search(self, query, params)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    serialize::to_bytes(self)
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    serialize::from_bytes(bytes)
  }

  /// Owned allocations, including nested vectors and shared vocabularies once.
  #[must_use]
  pub fn native_bytes(&self) -> usize {
    let mut seen = BTreeSet::new();
    let vocabulary_bytes = self
      .lexicons
      .iter()
      .filter(|lexicon| seen.insert(Arc::as_ptr(lexicon)))
      .fold(0_usize, |bytes, lexicon| {
        bytes
          .saturating_add(lexicon.native_bytes())
          .saturating_add(2_usize.saturating_mul(size_of::<usize>()))
      });
    size_of::<Self>()
      .saturating_add(
        self
          .lexicons
          .capacity()
          .saturating_mul(size_of::<Arc<Lexicon>>()),
      )
      .saturating_add(vocabulary_bytes)
      .saturating_add(
        self
          .aliases
          .capacity()
          .saturating_mul(size_of::<AliasInput>()),
      )
      .saturating_add(forward::nested_bytes(&self.aliases))
      .saturating_add(
        self
          .postings
          .native_bytes()
          .saturating_sub(size_of::<Postings>()),
      )
      .saturating_add(
        self
          .token_weights
          .capacity()
          .saturating_mul(size_of::<Vec<f64>>()),
      )
      .saturating_add(self.token_weights.iter().fold(
        0_usize,
        |bytes, weights| {
          bytes
            .saturating_add(weights.capacity().saturating_mul(size_of::<f64>()))
        },
      ))
      .saturating_add(self.options.budget.by_length.capacity())
  }

  #[must_use]
  pub const fn posting_count(&self) -> u64 {
    self.postings.len()
  }
}

impl SearchIndex for EntityIndex {
  fn minimum_token_weight(&self) -> f64 {
    MIN_WEIGHT
  }
  fn maximum_alias_tokens(&self) -> usize {
    self.maximum_alias_tokens
  }
  fn options(&self) -> &EntityIndexOptions {
    &self.options
  }
  fn alias_count(&self) -> usize {
    self.aliases.len()
  }
  fn alias(&self, id: AliasId) -> Option<&AliasInput> {
    self.aliases.get(usize::try_from(id).ok()?)
  }
  fn lexicon(&self, form: FormId) -> Option<&Lexicon> {
    self
      .lexicons
      .get(usize::try_from(form).ok()?)
      .map(Arc::as_ref)
  }
  fn posting(
    &self,
    list: PostingList,
    offset: usize,
  ) -> Option<(AliasId, f64)> {
    self.postings.get(list, offset)
  }
}
