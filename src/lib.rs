use std::panic;

use napi::bindgen_prelude::{
  AsyncTask, Error, Float64Array, Result, Uint8Array, Uint32Array,
};
use napi::{Env, Task};
use napi_derive::napi;
use stella_fuzzy_search_core as core;

fn core_to_napi_error(error: &core::Error) -> Error {
  Error::from_reason(error.to_string())
}

fn panic_to_napi_error(payload: &(dyn std::any::Any + Send)) -> Error {
  let msg = payload
    .downcast_ref::<&str>()
    .copied()
    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
    .unwrap_or("unknown panic");
  Error::from_reason(format!("Rust panic: {msg}"))
}

fn u32_overflow_error() -> Error {
  Error::from_reason(String::from("Result offset exceeds u32 range"))
}

/// A pattern entry for fuzzy search.
#[napi(object)]
pub struct PatternEntry {
  /// The pattern string to search for.
  pub pattern: String,
  /// Maximum edit distance (1-3). Default: 1.
  pub distance: Option<u8>,
  /// Optional name for the pattern.
  pub name: Option<String>,
}

/// Distance metric for fuzzy matching.
#[napi(string_enum)]
pub enum Metric {
  /// Standard Levenshtein: insertions, deletions,
  /// substitutions.
  #[napi(value = "levenshtein")]
  Levenshtein,
  /// Damerau-Levenshtein: insertions, deletions,
  /// substitutions, and transpositions of
  /// adjacent characters.
  #[napi(value = "damerau-levenshtein")]
  DamerauLevenshtein,
}

/// Options for constructing a `FuzzySearch`.
#[napi(object)]
pub struct Options {
  /// Distance metric. Default: `"levenshtein"`.
  pub metric: Option<Metric>,
  /// Strip diacritics before matching (NFD
  /// decompose + remove combining marks).
  /// Default: `false`.
  pub normalize_diacritics: Option<bool>,
  /// Use Unicode word boundaries.
  /// Default: `true`.
  pub unicode_boundaries: Option<bool>,
  /// Only match whole words. Default: `true`.
  pub whole_words: Option<bool>,
  /// Case-insensitive matching. Default: `false`.
  pub case_insensitive: Option<bool>,
}

/// A single fuzzy match (packed representation).
#[napi(object)]
pub struct FuzzyMatch {
  /// Index into the patterns array.
  pub pattern: u32,
  /// Start offset (UTF-16 code units).
  pub start: u32,
  /// End offset (exclusive, UTF-16 code units).
  pub end: u32,
  /// Actual edit distance of the match.
  pub distance: u32,
}

#[napi(js_name = "distance")]
#[allow(clippy::needless_pass_by_value)]
#[must_use]
/// Compute edit distance between two strings.
/// Uses Unicode characters (not UTF-16 code
/// units), so emoji and supplementary plane
/// characters are handled correctly.
///
/// `metric`: `"levenshtein"` (default) or
/// `"damerau-levenshtein"` (transpositions).
pub fn napi_distance(a: String, b: String, metric: Option<Metric>) -> u32 {
  let core_metric = match metric {
    Some(Metric::DamerauLevenshtein) => core::Metric::DamerauLevenshtein,
    Some(Metric::Levenshtein) | None => core::Metric::Levenshtein,
  };
  core::distance(&a, &b, core_metric)
}

fn resolve_options(options: Option<Options>) -> core::Options {
  let Some(opts) = options else {
    return core::Options::builder().build();
  };

  core::Options::builder()
    .metric(match opts.metric {
      Some(Metric::DamerauLevenshtein) => core::Metric::DamerauLevenshtein,
      Some(Metric::Levenshtein) | None => core::Metric::Levenshtein,
    })
    .normalize_diacritics(opts.normalize_diacritics.unwrap_or(false))
    .unicode_boundaries(opts.unicode_boundaries.unwrap_or(true))
    .whole_words(opts.whole_words.unwrap_or(true))
    .case_insensitive(opts.case_insensitive.unwrap_or(false))
    .build()
}

fn resolve_patterns(patterns: Vec<PatternEntry>) -> Vec<core::PatternEntry> {
  patterns
    .into_iter()
    .map(|pattern| core::PatternEntry {
      pattern: pattern.pattern,
      distance: pattern.distance,
    })
    .collect()
}

/// Fuzzy string matcher. Finds approximate
/// matches within edit distance k, immune to
/// typos, OCR errors, and diacritics variants.
///
/// Pattern names are handled in the JS wrapper
/// (not stored here).
#[napi]
pub struct FuzzySearch {
  inner: core::FuzzySearch,
}

#[napi]
#[allow(clippy::needless_pass_by_value)]
impl FuzzySearch {
  /// Build a fuzzy matcher from the given
  /// patterns and options.
  #[napi(constructor)]
  pub fn new(
    patterns: Vec<PatternEntry>,
    options: Option<Options>,
  ) -> Result<Self> {
    let inner = match panic::catch_unwind(|| {
      core::FuzzySearch::new(
        resolve_patterns(patterns),
        resolve_options(options),
      )
    }) {
      Ok(Ok(inner)) => inner,
      Ok(Err(error)) => return Err(core_to_napi_error(&error)),
      Err(error) => return Err(panic_to_napi_error(error.as_ref())),
    };
    Ok(Self { inner })
  }

  /// Number of patterns in the matcher.
  #[napi(getter)]
  #[must_use]
  pub const fn pattern_count(&self) -> u32 {
    self.inner.pattern_count()
  }

  /// Returns `true` if any pattern matches
  /// within its edit distance.
  #[napi]
  pub fn is_match(&self, haystack: String) -> Result<bool> {
    self
      .inner
      .is_match(&haystack)
      .map_err(|error| core_to_napi_error(&error))
  }

  /// Find all fuzzy matches. Returns a packed
  /// `Uint32Array` of `[pattern, start, end,
  /// distance]` quads. The JS wrapper unpacks
  /// these into `FuzzyMatch` objects.
  #[napi(js_name = "_findIterPacked")]
  pub fn find_iter_packed(&self, haystack: String) -> Result<Uint32Array> {
    self
      .inner
      .find_iter_packed(&haystack)
      .map(Uint32Array::new)
      .map_err(|error| core_to_napi_error(&error))
  }

  /// Replace all fuzzy matches.
  /// `replacements[i]` replaces pattern `i`.
  #[napi]
  pub fn replace_all(
    &self,
    haystack: String,
    replacements: Vec<String>,
  ) -> Result<String> {
    let expected_replacements = usize::try_from(self.inner.pattern_count())
      .map_err(|_| u32_overflow_error())?;
    if replacements.len() != expected_replacements {
      return Err(Error::from_reason(format!(
        "Expected {} replacements, got {}",
        self.inner.pattern_count(),
        replacements.len()
      )));
    }
    self
      .inner
      .replace_all(&haystack, &replacements)
      .map_err(|error| core_to_napi_error(&error))
  }
}

/// Indexed entity construction options. Text and weights are caller supplied.
#[napi(object)]
pub struct EntityIndexOptions {
  pub forms: u32,
  pub budget: EntityEditBudget,
  pub initial_weight: f64,
  pub initial_similarity: f64,
}

#[napi(object)]
pub struct EntityEditBudget {
  pub by_length: Vec<u8>,
  pub basis: EntityBudgetBasis,
}

#[napi(string_enum)]
pub enum EntityBudgetBasis {
  #[napi(value = "shorter")]
  Shorter,
}

#[napi(object)]
pub struct EntityAliasToken {
  pub id: u32,
  pub weight: f64,
  pub minimum_weight: f64,
}

impl From<EntityAliasToken> for core::entity::AliasToken {
  fn from(value: EntityAliasToken) -> Self {
    Self {
      id: value.id,
      weight: value.weight,
      minimum_weight: value.minimum_weight,
    }
  }
}

#[napi(object)]
pub struct EntityAliasJoin {
  pub id: u32,
  pub left: u32,
  pub right: u32,
}

impl From<EntityAliasJoin> for core::entity::AliasJoin {
  fn from(value: EntityAliasJoin) -> Self {
    Self {
      id: value.id,
      left: value.left,
      right: value.right,
    }
  }
}

#[napi(object)]
pub struct EntityAliasForm {
  pub form: u32,
  pub tokens: Vec<EntityAliasToken>,
  pub joins: Vec<EntityAliasJoin>,
  pub minimum_total: f64,
}

impl From<EntityAliasForm> for core::entity::AliasForm {
  fn from(value: EntityAliasForm) -> Self {
    Self {
      form: value.form,
      tokens: value.tokens.into_iter().map(Into::into).collect(),
      joins: value.joins.into_iter().map(Into::into).collect(),
      minimum_total: value.minimum_total,
    }
  }
}

#[napi(object)]
pub struct EntityAliasInput {
  pub entry_id: u32,
  pub entity_group: u32,
  pub quality: f64,
  pub forms: Vec<EntityAliasForm>,
}

impl From<EntityAliasInput> for core::entity::AliasInput {
  fn from(value: EntityAliasInput) -> Self {
    Self {
      entry_id: value.entry_id,
      entity_group: value.entity_group,
      quality: value.quality,
      forms: value.forms.into_iter().map(Into::into).collect(),
    }
  }
}

#[napi(object)]
pub struct EntityQueryToken {
  pub form: u32,
  pub text: String,
  pub weight: f64,
}

impl From<EntityQueryToken> for core::entity::QueryToken {
  fn from(value: EntityQueryToken) -> Self {
    Self {
      form: value.form,
      text: value.text,
      weight: value.weight,
    }
  }
}

#[napi(object)]
pub struct EntityQueryUnit {
  pub form: u32,
  pub text: String,
  pub positions: Vec<u32>,
}

impl From<EntityQueryUnit> for core::entity::QueryUnit {
  fn from(value: EntityQueryUnit) -> Self {
    Self {
      form: value.form,
      text: value.text,
      positions: value.positions,
    }
  }
}

#[napi(object)]
pub struct EntityQuery {
  pub tokens: Vec<EntityQueryToken>,
  pub units: Vec<EntityQueryUnit>,
}

impl From<EntityQuery> for core::entity::EntityQuery {
  fn from(value: EntityQuery) -> Self {
    Self {
      tokens: value.tokens.into_iter().map(Into::into).collect(),
      units: value.units.into_iter().map(Into::into).collect(),
    }
  }
}

#[napi(object)]
pub struct EntitySearchParams {
  pub tau: f64,
  pub quality: f64,
  pub max_cost: f64,
}

/// Parallel typed arrays: matchKeys contains [unit, form, token] triples.
#[napi(object)]
pub struct EntitySearchResult {
  pub match_keys: Uint32Array,
  pub similarities: Float64Array,
  pub query_weights: Float64Array,
  pub survivors: Uint32Array,
  /// [lexiconCells, lexiconMatches, sortedAccesses, randomAccessPairs, survivors].
  pub cost: Float64Array,
  pub exhausted: bool,
}

const MAX_SAFE_INTEGER: u64 = 0x001F_FFFF_FFFF_FFFF;

// Integers in the checked range are represented exactly by JavaScript numbers.
#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
fn safe_number(value: u64) -> Result<f64> {
  if value > MAX_SAFE_INTEGER {
    return Err(Error::from_reason(
      "Entity count exceeds JavaScript safe integer range",
    ));
  }
  Ok(value as f64)
}

// The finite, integral safe-integer check bounds this conversion.
#[allow(
  clippy::as_conversions,
  clippy::cast_possible_truncation,
  clippy::cast_sign_loss
)]
fn work_limit(value: f64) -> Result<u64> {
  if !value.is_finite()
    || value < 0.0
    || value.fract() != 0.0
    || value > 9_007_199_254_740_991.0
  {
    return Err(Error::from_reason(
      "maxCost must be a non-negative safe integer",
    ));
  }
  Ok(value as u64)
}

fn pack_entity_result(
  result: core::entity::SearchResult,
) -> Result<EntitySearchResult> {
  let count = result.unit_matches.len();
  let mut keys = Vec::with_capacity(count.saturating_mul(3));
  let mut similarities = Vec::with_capacity(count);
  for hit in result.unit_matches {
    keys.extend([hit.unit, hit.form, hit.id]);
    similarities.push(hit.similarity);
  }
  let cost = result.cost;
  Ok(EntitySearchResult {
    match_keys: Uint32Array::new(keys),
    similarities: Float64Array::new(similarities),
    query_weights: Float64Array::new(result.query_weights),
    survivors: Uint32Array::new(result.survivors),
    cost: Float64Array::new(vec![
      safe_number(cost.lexicon_cells)?,
      safe_number(cost.lexicon_matches)?,
      safe_number(cost.sorted_accesses)?,
      safe_number(cost.random_access_pairs)?,
      safe_number(cost.survivors)?,
    ]),
    exhausted: result.exhausted,
  })
}

/// A builder is consumed by finish or finishAsync, including failed finishes.
#[napi]
pub struct EntityIndexBuilder {
  inner: Option<core::entity::EntityIndexBuilder>,
}

impl EntityIndexBuilder {
  fn active(&mut self) -> Result<&mut core::entity::EntityIndexBuilder> {
    self.inner.as_mut().ok_or_else(|| {
      Error::from_reason("Entity builder has already been consumed")
    })
  }

  fn take(&mut self) -> Result<core::entity::EntityIndexBuilder> {
    self.inner.take().ok_or_else(|| {
      Error::from_reason("Entity builder has already been consumed")
    })
  }
}

// NAPI decodes these boundary arguments into owned values.
#[napi]
#[allow(clippy::needless_pass_by_value)]
impl EntityIndexBuilder {
  #[napi(constructor)]
  pub fn new(options: EntityIndexOptions) -> Result<Self> {
    let basis = match options.budget.basis {
      EntityBudgetBasis::Shorter => core::entity::BudgetBasis::Shorter,
    };
    let options = core::entity::EntityIndexOptions {
      forms: options.forms,
      budget: core::entity::EditBudget {
        by_length: options.budget.by_length,
        basis,
      },
      initial_weight: options.initial_weight,
      initial_similarity: options.initial_similarity,
    };
    let inner = match panic::catch_unwind(|| {
      core::entity::EntityIndexBuilder::new(options)
    }) {
      Ok(Ok(inner)) => inner,
      Ok(Err(error)) => return Err(core_to_napi_error(&error)),
      Err(error) => return Err(panic_to_napi_error(error.as_ref())),
    };
    Ok(Self { inner: Some(inner) })
  }

  #[napi]
  pub fn intern(&mut self, form: u32, text: String) -> Result<u32> {
    self
      .active()?
      .intern(form, &text)
      .map_err(|e| core_to_napi_error(&e))
  }

  #[napi]
  pub fn add_alias(&mut self, alias: EntityAliasInput) -> Result<u32> {
    self
      .active()?
      .add_alias(alias.into())
      .map_err(|e| core_to_napi_error(&e))
  }

  #[napi]
  pub fn finish_step(&mut self, max_units: u32) -> Result<bool> {
    self
      .active()?
      .finish_step(max_units)
      .map_err(|e| core_to_napi_error(&e))
  }

  #[napi]
  pub fn finish(
    &mut self,
    token_weights: Vec<Float64Array>,
  ) -> Result<EntityIndex> {
    let weights = token_weights.into_iter().map(|w| w.to_vec()).collect();
    let inner = self
      .take()?
      .finish(weights)
      .map_err(|e| core_to_napi_error(&e))?;
    Ok(EntityIndex { inner })
  }

  #[napi(ts_return_type = "Promise<EntityIndex>")]
  pub fn finish_async(
    &mut self,
    token_weights: Vec<Float64Array>,
  ) -> Result<AsyncTask<EntityFinishTask>> {
    let weights = token_weights.into_iter().map(|w| w.to_vec()).collect();
    Ok(AsyncTask::new(EntityFinishTask {
      builder: Some(self.take()?),
      weights: Some(weights),
    }))
  }
}

pub struct EntityFinishTask {
  builder: Option<core::entity::EntityIndexBuilder>,
  weights: Option<Vec<Vec<f64>>>,
}

impl Task for EntityFinishTask {
  type Output = core::entity::EntityIndex;
  type JsValue = EntityIndex;

  fn compute(&mut self) -> Result<Self::Output> {
    let builder = self.builder.take().ok_or_else(|| {
      Error::from_reason("Entity finish task already executed")
    })?;
    let weights = self.weights.take().ok_or_else(|| {
      Error::from_reason("Entity finish task already executed")
    })?;
    match panic::catch_unwind(panic::AssertUnwindSafe(|| {
      builder.finish(weights)
    })) {
      Ok(result) => result.map_err(|e| core_to_napi_error(&e)),
      Err(error) => Err(panic_to_napi_error(error.as_ref())),
    }
  }

  fn resolve(
    &mut self,
    _env: Env,
    inner: Self::Output,
  ) -> Result<Self::JsValue> {
    Ok(EntityIndex { inner })
  }
}

#[napi]
pub struct EntityIndex {
  inner: core::entity::EntityIndex,
}

// NAPI decodes these boundary arguments into owned values.
#[napi]
#[allow(clippy::needless_pass_by_value)]
impl EntityIndex {
  #[napi(factory)]
  pub fn from_bytes(bytes: Uint8Array) -> Result<Self> {
    let inner = core::entity::EntityIndex::from_bytes(&bytes)
      .map_err(|e| core_to_napi_error(&e))?;
    Ok(Self { inner })
  }

  #[napi]
  pub fn to_bytes(&self) -> Result<Uint8Array> {
    self
      .inner
      .to_bytes()
      .map(Uint8Array::new)
      .map_err(|e| core_to_napi_error(&e))
  }

  #[napi(getter)]
  pub fn native_bytes(&self) -> Result<f64> {
    let bytes = u64::try_from(self.inner.native_bytes())
      .map_err(|_| u32_overflow_error())?;
    safe_number(bytes)
  }

  #[napi(getter)]
  pub fn posting_count(&self) -> Result<f64> {
    safe_number(self.inner.posting_count())
  }

  #[napi(js_name = "_searchPacked")]
  pub fn search_packed(
    &self,
    query: EntityQuery,
    params: EntitySearchParams,
  ) -> Result<EntitySearchResult> {
    let query = query.into();
    let params = core::entity::SearchParams {
      tau: params.tau,
      quality: params.quality,
      max_cost: work_limit(params.max_cost)?,
    };
    let result = self
      .inner
      .search(&query, &params)
      .map_err(|e| core_to_napi_error(&e))?;
    pack_entity_result(result)
  }
}
