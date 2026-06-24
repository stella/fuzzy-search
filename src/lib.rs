use std::panic;

use napi::bindgen_prelude::{Error, Result, Uint32Array};
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
  let opts = options.unwrap_or(Options {
    metric: None,
    normalize_diacritics: None,
    unicode_boundaries: None,
    whole_words: None,
    case_insensitive: None,
  });
  core::Options {
    metric: match opts.metric {
      Some(Metric::DamerauLevenshtein) => core::Metric::DamerauLevenshtein,
      Some(Metric::Levenshtein) | None => core::Metric::Levenshtein,
    },
    normalize_diacritics: opts.normalize_diacritics.unwrap_or(false),
    unicode_boundaries: opts.unicode_boundaries.unwrap_or(true),
    whole_words: opts.whole_words.unwrap_or(true),
    case_insensitive: opts.case_insensitive.unwrap_or(false),
  }
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
