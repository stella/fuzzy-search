use std::collections::HashMap;
use std::panic;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use unicode_normalization::char::decompose_canonical;
use unicode_normalization::char::is_combining_mark;

/// Unicode Simple Case Fold (CaseFolding.txt S/C)
/// plus Turkic İ→i. Always 1:1 character mapping.
/// See `@stll/aho-corasick` for detailed rationale.
#[inline]
fn simple_case_fold(ch: char) -> char {
  match ch {
    '\u{0130}' => 'i', // İ → i (Turkic, not in S/C)
    _ => unicode_case_mapping::case_folded(ch)
      .and_then(|n| char::from_u32(n.get()))
      .unwrap_or(ch),
  }
}

/// Convert a caught panic into a napi `Error`.
fn panic_to_napi_error(
  payload: Box<dyn std::any::Any + Send>,
) -> Error {
  let msg = payload
    .downcast_ref::<&str>()
    .copied()
    .or_else(|| {
      payload.downcast_ref::<String>().map(|s| s.as_str())
    })
    .unwrap_or("unknown panic");
  Error::from_reason(format!("Rust panic: {msg}"))
}

// ─── NAPI types ──────────────────────────────

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

// ─── Word boundary detection ─────────────────
//
// Two modes:
// 1. Inline: is_alphanumeric() + CJK exception.
//    Fast, correct for Latin/Cyrillic/Greek/etc.
// 2. UAX#29: unicode-segmentation crate. Correct
//    for Thai, Lao, Khmer, Myanmar (no inter-word
//    spaces). Activated automatically when the
//    haystack contains these scripts.

fn is_cjk(ch: char) -> bool {
  matches!(u32::from(ch),
    0x3040..=0x309F   // Hiragana
    | 0x30A0..=0x30FF // Katakana
    | 0x3400..=0x4DBF // CJK Extension A
    | 0x4E00..=0x9FFF // CJK Unified Ideographs
    | 0xAC00..=0xD7AF // Hangul Syllables
    | 0xF900..=0xFAFF // CJK Compatibility
    | 0x20000..=0x2FA1F // CJK Extensions B-F
    | 0x30000..=0x323AF // CJK Extensions G-I
  )
}

fn is_word_char(ch: char) -> bool {
  ch.is_alphanumeric() && !is_cjk(ch)
}

/// Inline boundary check on char slices.
fn is_whole_word_inline(
  chars: &[char],
  start: usize,
  end: usize,
) -> bool {
  if start >= end || end > chars.len() {
    return false;
  }
  let start_ok = start == 0
    || !is_word_char(chars[start - 1])
    || is_cjk(chars[start]);
  let end_ok = end >= chars.len()
    || !is_word_char(chars[end])
    || is_cjk(chars[end - 1]);
  start_ok && end_ok
}

// ─── UAX#29 segmenter ───────────────────────
//
// For scripts without inter-word spaces (Thai,
// Lao, Khmer, Myanmar), pre-compute word
// boundaries using the unicode-segmentation
// crate. Stores boundaries as a bit set indexed
// by char position for O(1) lookup.

/// Does the text contain scripts that need
/// UAX#29 segmentation?
fn needs_segmenter(text: &str) -> bool {
  if text.is_ascii() {
    return false;
  }
  for ch in text.chars() {
    let cp = u32::from(ch);
    if (0x0E00..=0x0E7F).contains(&cp)    // Thai
      || (0x0E80..=0x0EFF).contains(&cp)  // Lao
      || (0x1000..=0x109F).contains(&cp)  // Myanmar
      || (0x1780..=0x17FF).contains(&cp)
    // Khmer
    {
      return true;
    }
  }
  false
}

/// Bit set for O(1) boundary lookups by char
/// index (not byte offset).
struct CharBoundarySet {
  bits: Vec<u64>,
}

impl CharBoundarySet {
  fn new(len: usize) -> Self {
    Self {
      bits: vec![0u64; len.div_ceil(64)],
    }
  }

  fn set(&mut self, pos: usize) {
    if pos < self.bits.len() * 64 {
      self.bits[pos / 64] |= 1u64 << (pos % 64);
    }
  }

  fn contains(&self, pos: usize) -> bool {
    pos < self.bits.len() * 64
      && self.bits[pos / 64] & (1u64 << (pos % 64)) != 0
  }
}

/// Compute UAX#29 word boundaries as char-index
/// positions (not byte offsets).
fn compute_char_boundaries(text: &str) -> CharBoundarySet {
  use unicode_segmentation::UnicodeSegmentation;
  // Build byte-offset → char-index map.
  let mut byte_to_char: Vec<usize> =
    Vec::with_capacity(text.len() + 1);
  let mut char_idx = 0;
  for ch in text.chars() {
    for _ in 0..ch.len_utf8() {
      byte_to_char.push(char_idx);
    }
    char_idx += 1;
  }
  byte_to_char.push(char_idx); // sentinel

  let mut bs = CharBoundarySet::new(char_idx + 1);
  bs.set(0);
  bs.set(char_idx);
  for (byte_off, word) in text.unicode_word_indices() {
    let start_char = byte_to_char[byte_off];
    let end_byte = byte_off + word.len();
    let end_char = byte_to_char[end_byte];
    bs.set(start_char);
    bs.set(end_char);
  }
  bs
}

/// Boundary mode: inline or UAX#29 segmenter.
enum BoundaryMode {
  Inline,
  Segmenter { bitset: CharBoundarySet },
}

impl BoundaryMode {
  fn is_whole_word(
    &self,
    chars: &[char],
    start: usize,
    end: usize,
  ) -> bool {
    match self {
      BoundaryMode::Inline => {
        is_whole_word_inline(chars, start, end)
      }
      BoundaryMode::Segmenter { bitset } => {
        if start >= end || end > chars.len() {
          return false;
        }
        bitset.contains(start) && bitset.contains(end)
      }
    }
  }
}

/// Choose boundary mode based on text content.
fn choose_boundary_mode(
  text: &str,
  unicode_boundaries: bool,
) -> BoundaryMode {
  if unicode_boundaries && needs_segmenter(text) {
    BoundaryMode::Segmenter {
      bitset: compute_char_boundaries(text),
    }
  } else {
    BoundaryMode::Inline
  }
}

// ─── Combining mark detection ────────────────
//
// After NFD decomposition, combining marks are
// stripped to normalize diacritics. Uses the
// unicode-normalization crate's `is_combining_mark`
// which checks Unicode General Category = Mark
// (Mn, Mc, Me) — correct for ALL scripts
// (Latin, Cyrillic, Devanagari, Thai, etc.).

// ─── Text normalization ──────────────────────
//
// Normalize text for matching: optional NFD
// diacritics stripping and case folding. Returns
// normalized characters and a position map from
// normalized index → original char index.

fn normalize_with_map(
  text: &str,
  strip_dia: bool,
  case_insensitive: bool,
) -> (Vec<char>, Vec<usize>) {
  let orig_chars: Vec<char> = text.chars().collect();
  let orig_len = orig_chars.len();

  if !strip_dia && !case_insensitive {
    let mut map: Vec<usize> = (0..orig_len).collect();
    map.push(orig_len); // sentinel
    return (orig_chars, map);
  }

  let mut norm = Vec::with_capacity(orig_len);
  let mut map = Vec::with_capacity(orig_len + 1);

  for (orig_idx, &ch) in orig_chars.iter().enumerate() {
    if strip_dia {
      decompose_canonical(ch, |dc| {
        if !is_combining_mark(dc) {
          if case_insensitive {
            norm.push(simple_case_fold(dc));
            map.push(orig_idx);
          } else {
            norm.push(dc);
            map.push(orig_idx);
          }
        }
      });
    } else {
      // case_insensitive only
      norm.push(simple_case_fold(ch));
      map.push(orig_idx);
    }
  }

  map.push(orig_len); // sentinel
  (norm, map)
}

// ─── Myers bit-parallel algorithm ────────────
//
// Semi-global fuzzy matching: finds all positions
// in the text where the pattern occurs within
// edit distance k. Based on Gene Myers' "A Fast
// Bit-Vector Algorithm for Approximate String
// Matching Based on Dynamic Programming" (1999).
//
// Returns end positions (exclusive, char indices)
// with their edit distances.

fn myers_find_ends(
  pattern: &[char],
  text: &[char],
  max_dist: u8,
) -> Vec<(usize, u8)> {
  let m = pattern.len();
  if m == 0 || m > 64 || text.is_empty() {
    return vec![];
  }
  let k = i32::from(max_dist);

  // Build pattern bitmasks: peq[c] has bit i
  // set iff pattern[i] == c.
  let mut peq: HashMap<char, u64> = HashMap::new();
  for (i, &c) in pattern.iter().enumerate() {
    *peq.entry(c).or_insert(0) |= 1u64 << i;
  }

  let mask =
    if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
  let msb = 1u64 << (m - 1);

  // PV = positive vertical deltas (all +1 init)
  // MV = negative vertical deltas (all 0 init)
  let mut pv: u64 = mask;
  let mut mv: u64 = 0;
  let mut score = m as i32;

  let mut results = Vec::new();

  for (j, &tc) in text.iter().enumerate() {
    let eq = peq.get(&tc).copied().unwrap_or(0);

    let xv = eq | mv;
    let xh = (((eq & pv).wrapping_add(pv)) ^ pv) | eq | mv;

    let ph = mv | !(xh | pv);
    let mh = pv & xh;

    // Update score from the m-th bit.
    if ph & msb != 0 {
      score += 1;
    }
    if mh & msb != 0 {
      score -= 1;
    }

    // Semi-global: no | 1 on ph shift (free
    // leading gaps in text).
    let ph_shifted = ph << 1;
    let mh_shifted = mh << 1;

    pv = (mh_shifted | !(xv | ph_shifted)) & mask;
    mv = (ph_shifted & xv) & mask;

    if score <= k {
      results.push((j + 1, score as u8));
    }
  }

  results
}

// ─── Distance functions ─────────────────────

/// Standard Levenshtein edit distance on char
/// slices. O(m × n) time, O(n) space.
fn levenshtein(a: &[char], b: &[char]) -> usize {
  let m = a.len();
  let n = b.len();
  if m == 0 {
    return n;
  }
  if n == 0 {
    return m;
  }

  let mut prev: Vec<usize> = (0..=n).collect();

  for i in 1..=m {
    let mut curr = vec![0usize; n + 1];
    curr[0] = i;
    for j in 1..=n {
      let cost = usize::from(a[i - 1] != b[j - 1]);
      curr[j] = (curr[j - 1] + 1)
        .min(prev[j] + 1)
        .min(prev[j - 1] + cost);
    }
    prev = curr;
  }
  prev[n]
}

/// Optimal String Alignment (restricted Damerau-
/// Levenshtein) on char slices. Counts adjacent
/// transpositions as a single edit.
fn damerau_levenshtein(a: &[char], b: &[char]) -> usize {
  let m = a.len();
  let n = b.len();
  if m == 0 {
    return n;
  }
  if n == 0 {
    return m;
  }

  // Need two previous rows for transposition.
  let mut prev2: Vec<usize> = vec![0; n + 1];
  let mut prev: Vec<usize> = (0..=n).collect();

  for i in 1..=m {
    let mut curr = vec![0usize; n + 1];
    curr[0] = i;
    for j in 1..=n {
      let cost = usize::from(a[i - 1] != b[j - 1]);
      curr[j] = (curr[j - 1] + 1)
        .min(prev[j] + 1)
        .min(prev[j - 1] + cost);
      // Transposition: if a[i-1]==b[j-2] and
      // a[i-2]==b[j-1], swapping is 1 edit.
      if i > 1
        && j > 1
        && a[i - 1] == b[j - 2]
        && a[i - 2] == b[j - 1]
      {
        curr[j] = curr[j].min(prev2[j - 2] + 1);
      }
    }
    prev2 = prev;
    prev = curr;
  }
  prev[n]
}

/// Dispatch to the correct distance function.
fn edit_distance(
  a: &[char],
  b: &[char],
  use_damerau: bool,
) -> usize {
  if use_damerau {
    damerau_levenshtein(a, b)
  } else {
    levenshtein(a, b)
  }
}

// ─── Start position finder ───────────────────
//
// Given an end position from Myers, find the
// exact start position by trying all valid
// window lengths [m-k, m+k] and computing
// Levenshtein distance for each.

fn find_start(
  pattern: &[char],
  text: &[char],
  end: usize,
  dist: u8,
  actual_max: u8,
  use_damerau: bool,
) -> Option<(usize, u8)> {
  let m = pattern.len();
  // `dist` determines the window range (from
  // Myers prefilter). `actual_max` is the real
  // distance threshold for the chosen metric.
  let k = dist as usize;
  let max_k = actual_max as usize;

  // Enforce min_len >= 1 to avoid zero-length
  // matches (e.g. pattern "ab" dist 2 matching "").
  let min_len = m.saturating_sub(k).max(1);
  let max_len = (m + k).min(end);

  // Try exact pattern length first (most common).
  if end >= m {
    let start = end - m;
    let d = edit_distance(
      pattern,
      &text[start..end],
      use_damerau,
    );
    if d <= max_k {
      return Some((start, d as u8));
    }
  }

  // Try shorter/longer windows.
  let mut best: Option<(usize, u8)> = None;
  for len in min_len..=max_len {
    if len == m {
      continue; // already tried
    }
    if end < len {
      continue;
    }
    let start = end - len;
    let d = edit_distance(
      pattern,
      &text[start..end],
      use_damerau,
    );
    if d <= max_k {
      match best {
        None => best = Some((start, d as u8)),
        Some((_, bd)) if (d as u8) < bd => {
          best = Some((start, d as u8));
        }
        _ => {}
      }
    }
  }
  best
}

// ─── Match region extraction ─────────────────
//
// From Myers end positions, extract local minima
// in the distance curve and compute start
// positions. Returns non-overlapping matches
// sorted by start position.

fn extract_matches(
  pattern: &[char],
  text: &[char],
  end_positions: &[(usize, u8)],
  window_dist: u8,
  actual_max: u8,
  use_damerau: bool,
) -> Vec<(usize, usize, u8)> {
  if end_positions.is_empty() {
    return vec![];
  }

  // Greedy left-to-right. For each end position,
  // look ahead in a window of m positions and
  // try find_start for each. Pick the best match
  // (lowest distance, then closest to pattern
  // length, then leftmost start).
  let m = pattern.len();
  let mut matches = Vec::new();
  let mut last_match_end: usize = 0;
  let mut i = 0;

  while i < end_positions.len() {
    let (end, _) = end_positions[i];

    // Evaluate candidates in a contiguous window.
    // Window extends end + 2m + k to ensure we
    // catch better matches further ahead (e.g.,
    // an exact match preceded by noisy text).
    let k = window_dist as usize;
    let window_bound = end + 2 * m + k;
    let mut best: Option<(usize, usize, u8)> = None;
    let mut best_end_idx = i;
    let mut j = i;
    while j < end_positions.len()
      && end_positions[j].0 <= window_bound
      && (j == i
        || end_positions[j].0 == end_positions[j - 1].0 + 1)
    {
      let (je, jd) = end_positions[j];
      if let Some((start, actual_dist)) = find_start(
        pattern,
        text,
        je,
        jd,
        actual_max,
        use_damerau,
      ) {
        if start >= last_match_end {
          let len = je - start;
          let len_diff = len.abs_diff(m);
          let is_better = match best {
            None => true,
            Some((bs, be, bd)) => {
              let bl = be - bs;
              let bl_diff = bl.abs_diff(m);
              actual_dist < bd
                || (actual_dist == bd && len_diff < bl_diff)
                || (actual_dist == bd
                  && len_diff == bl_diff
                  && start < bs)
            }
          };
          if is_better {
            best = Some((start, je, actual_dist));
            best_end_idx = j;
          }
        }
      }
      j += 1;
    }

    if let Some((start, be, dist)) = best {
      matches.push((start, be, dist));
      last_match_end = be;
      // Skip past this match.
      i = best_end_idx + 1;
      while i < end_positions.len()
        && end_positions[i].0 <= be
      {
        i += 1;
      }
    } else {
      i += 1;
    }
  }

  matches
}

// ─── Standalone distance function ────────────

/// Compute edit distance between two strings.
/// Uses Unicode characters (not UTF-16 code
/// units), so emoji and supplementary plane
/// characters are handled correctly.
///
/// `metric`: `"levenshtein"` (default) or
/// `"damerau-levenshtein"` (transpositions).
#[napi(js_name = "distance")]
pub fn napi_distance(
  a: String,
  b: String,
  metric: Option<Metric>,
) -> u32 {
  let ac: Vec<char> = a.chars().collect();
  let bc: Vec<char> = b.chars().collect();
  let use_damerau =
    matches!(metric, Some(Metric::DamerauLevenshtein));
  edit_distance(&ac, &bc, use_damerau) as u32
}

// ─── UTF-16 offset mapping ──────────────────

/// Build a char-index → UTF-16 code unit offset
/// mapping. Index `i` gives the UTF-16 offset of
/// char `i`; index `len` is the total length.
fn build_utf16_map(chars: &[char]) -> Vec<u32> {
  let mut map = Vec::with_capacity(chars.len() + 1);
  let mut utf16_pos: u32 = 0;
  for &ch in chars {
    map.push(utf16_pos);
    utf16_pos += ch.len_utf16() as u32;
  }
  map.push(utf16_pos);
  map
}

// ─── FuzzySearch ─────────────────────────────

/// Preprocessed pattern for fuzzy matching.
struct PatternInfo {
  /// Normalized pattern as chars.
  chars: Vec<char>,
  /// Maximum edit distance.
  max_dist: u8,
}

/// Fuzzy string matcher. Finds approximate
/// matches within edit distance k, immune to
/// typos, OCR errors, and diacritics variants.
///
/// Pattern names are handled in the JS wrapper
/// (not stored here).
#[napi]
pub struct FuzzySearch {
  patterns: Vec<PatternInfo>,
  normalize_diacritics: bool,
  case_insensitive: bool,
  whole_words: bool,
  unicode_boundaries: bool,
  use_damerau: bool,
  pattern_count: u32,
}

fn default_options() -> Options {
  Options {
    metric: None,
    normalize_diacritics: None,
    unicode_boundaries: None,
    whole_words: None,
    case_insensitive: None,
  }
}

#[napi]
impl FuzzySearch {
  /// Build a fuzzy matcher from the given
  /// patterns and options.
  #[napi(constructor)]
  pub fn new(
    patterns: Vec<PatternEntry>,
    options: Option<Options>,
  ) -> Result<Self> {
    panic::catch_unwind(|| {
      Self::new_inner(patterns, options)
    })
    .unwrap_or_else(|e| Err(panic_to_napi_error(e)))
  }

  fn new_inner(
    patterns: Vec<PatternEntry>,
    options: Option<Options>,
  ) -> Result<Self> {
    let opts = options.unwrap_or_else(default_options);
    let normalize =
      opts.normalize_diacritics.unwrap_or(false);
    let case_insensitive =
      opts.case_insensitive.unwrap_or(false);
    let whole_words = opts.whole_words.unwrap_or(true);
    let unicode_boundaries =
      opts.unicode_boundaries.unwrap_or(true);
    let use_damerau = matches!(
      opts.metric,
      Some(Metric::DamerauLevenshtein)
    );
    let pattern_count = patterns.len() as u32;

    let mut infos = Vec::with_capacity(patterns.len());

    for p in patterns {
      let dist = p.distance.unwrap_or(1);
      // Myers is O(n) regardless of distance,
      // so no hard upper limit. But distance >=
      // pattern length means nearly everything
      // matches (useless noise).
      let (chars, _) = normalize_with_map(
        &p.pattern,
        normalize,
        case_insensitive,
      );
      if chars.is_empty() {
        return Err(Error::from_reason(
          "Empty pattern".to_string(),
        ));
      }
      if chars.len() > 64 {
        return Err(Error::from_reason(
          "Pattern too long (max 64 chars)".to_string(),
        ));
      }
      // Myers is O(n) regardless of distance,
      // so no hard upper limit. But distance >=
      // pattern length means nearly everything
      // matches (useless noise).
      if dist as usize >= chars.len() {
        return Err(Error::from_reason(format!(
          "Distance {} >= pattern length {} \
           (every substring would match)",
          dist,
          chars.len(),
        )));
      }
      infos.push(PatternInfo {
        chars,
        max_dist: dist,
      });
    }

    Ok(Self {
      patterns: infos,
      normalize_diacritics: normalize,
      case_insensitive,
      whole_words,
      unicode_boundaries,
      use_damerau,
      pattern_count,
    })
  }

  /// Number of patterns in the matcher.
  #[napi(getter)]
  pub fn pattern_count(&self) -> u32 {
    self.pattern_count
  }

  /// Find end positions. For Damerau, run Myers
  /// with expanded distance (2k) as a prefilter
  /// since Levenshtein(a,b) <= 2 * Damerau(a,b).
  /// The actual Damerau distance is computed in
  /// find_start during verification.
  fn find_ends(
    &self,
    pattern: &[char],
    text: &[char],
    max_dist: u8,
  ) -> Vec<(usize, u8)> {
    if self.use_damerau {
      // Conservative prefilter: any Damerau-k
      // match has Levenshtein distance <= 2k.
      let prefilter_dist = (max_dist as usize * 2)
        .min(pattern.len().saturating_sub(1))
        as u8;
      myers_find_ends(pattern, text, prefilter_dist)
    } else {
      myers_find_ends(pattern, text, max_dist)
    }
  }

  /// Dispatch extract_matches with metric.
  fn extract(
    &self,
    pattern: &[char],
    text: &[char],
    ends: &[(usize, u8)],
    max_dist: u8,
  ) -> Vec<(usize, usize, u8)> {
    // For Damerau: use expanded window for
    // candidate search, but filter by actual
    // max_dist via the distance function.
    let window_dist = if self.use_damerau {
      (max_dist as usize * 2)
        .min(pattern.len().saturating_sub(1)) as u8
    } else {
      max_dist
    };
    extract_matches(
      pattern,
      text,
      ends,
      window_dist,
      max_dist,
      self.use_damerau,
    )
  }

  /// Returns `true` if any pattern matches
  /// within its edit distance.
  #[napi]
  pub fn is_match(&self, haystack: String) -> bool {
    let orig_chars: Vec<char> = haystack.chars().collect();
    let boundary = choose_boundary_mode(
      &haystack,
      self.unicode_boundaries,
    );
    let (text_chars, pos_map) = normalize_with_map(
      &haystack,
      self.normalize_diacritics,
      self.case_insensitive,
    );

    for pat in &self.patterns {
      let ends = self.find_ends(
        &pat.chars,
        &text_chars,
        pat.max_dist,
      );
      let matches = self.extract(
        &pat.chars,
        &text_chars,
        &ends,
        pat.max_dist,
      );
      for (start, end, _) in matches {
        if !self.whole_words {
          return true;
        }
        let orig_start = pos_map[start];
        let orig_end = pos_map[end];
        if boundary.is_whole_word(
          &orig_chars,
          orig_start,
          orig_end,
        ) {
          return true;
        }
      }
    }
    false
  }

  /// Find all fuzzy matches. Returns a packed
  /// `Uint32Array` of `[pattern, start, end,
  /// distance]` quads. The JS wrapper unpacks
  /// these into `FuzzyMatch` objects.
  #[napi(js_name = "_findIterPacked")]
  pub fn find_iter_packed(
    &self,
    haystack: String,
  ) -> Uint32Array {
    let orig_chars: Vec<char> = haystack.chars().collect();
    let utf16_map = build_utf16_map(&orig_chars);
    let boundary = choose_boundary_mode(
      &haystack,
      self.unicode_boundaries,
    );
    let (text_chars, pos_map) = normalize_with_map(
      &haystack,
      self.normalize_diacritics,
      self.case_insensitive,
    );

    let mut all: Vec<(u32, u32, u32, u32)> = Vec::new();

    for (idx, pat) in self.patterns.iter().enumerate() {
      let ends = self.find_ends(
        &pat.chars,
        &text_chars,
        pat.max_dist,
      );
      let matches = self.extract(
        &pat.chars,
        &text_chars,
        &ends,
        pat.max_dist,
      );

      for (start, end, dist) in matches {
        let orig_start = pos_map[start];
        let orig_end = pos_map[end];

        if self.whole_words
          && !boundary.is_whole_word(
            &orig_chars,
            orig_start,
            orig_end,
          )
        {
          continue;
        }

        let utf16_start = utf16_map[orig_start];
        let utf16_end = utf16_map[orig_end];
        all.push((
          idx as u32,
          utf16_start,
          utf16_end,
          u32::from(dist),
        ));
      }
    }

    // Sort by start position, then distance
    // (prefer lower), then longer match.
    all.sort_unstable_by(|a, b| {
      a.1.cmp(&b.1).then(a.3.cmp(&b.3)).then(b.2.cmp(&a.2))
    });

    // Greedy non-overlapping across all patterns.
    let mut packed = Vec::with_capacity(all.len() * 4);
    let mut last_end: u32 = 0;
    for (pat, start, end, dist) in all {
      if start < last_end {
        continue;
      }
      packed.push(pat);
      packed.push(start);
      packed.push(end);
      packed.push(dist);
      last_end = end;
    }

    Uint32Array::new(packed)
  }

  /// Replace all fuzzy matches.
  /// `replacements[i]` replaces pattern `i`.
  #[napi]
  pub fn replace_all(
    &self,
    haystack: String,
    replacements: Vec<String>,
  ) -> Result<String> {
    if replacements.len() != self.pattern_count as usize {
      return Err(Error::from_reason(format!(
        "Expected {} replacements, got {}",
        self.pattern_count,
        replacements.len()
      )));
    }

    let orig_chars: Vec<char> = haystack.chars().collect();
    let boundary = choose_boundary_mode(
      &haystack,
      self.unicode_boundaries,
    );
    let (text_chars, pos_map) = normalize_with_map(
      &haystack,
      self.normalize_diacritics,
      self.case_insensitive,
    );

    // Collect all matches across patterns.
    // (start, end, pat_idx, distance)
    let mut all: Vec<(usize, usize, u32, u8)> = Vec::new();

    for (idx, pat) in self.patterns.iter().enumerate() {
      let ends = self.find_ends(
        &pat.chars,
        &text_chars,
        pat.max_dist,
      );
      let matches = self.extract(
        &pat.chars,
        &text_chars,
        &ends,
        pat.max_dist,
      );

      for (start, end, dist) in matches {
        let orig_start = pos_map[start];
        let orig_end = pos_map[end];

        if self.whole_words
          && !boundary.is_whole_word(
            &orig_chars,
            orig_start,
            orig_end,
          )
        {
          continue;
        }
        all.push((orig_start, orig_end, idx as u32, dist));
      }
    }

    // Sort same as find_iter_packed: start, then
    // distance (prefer lower), then longer match.
    all.sort_unstable_by(|a, b| {
      a.0.cmp(&b.0).then(a.3.cmp(&b.3)).then(b.1.cmp(&a.1))
    });

    // Build result, replacing non-overlapping
    // matches.
    let mut result = String::with_capacity(haystack.len());
    let mut pos: usize = 0;

    for (start, end, pat_idx, _) in &all {
      if *start < pos {
        continue; // skip overlapping
      }
      for &ch in &orig_chars[pos..*start] {
        result.push(ch);
      }
      result.push_str(&replacements[*pat_idx as usize]);
      pos = *end;
    }
    for &ch in &orig_chars[pos..] {
      result.push(ch);
    }

    Ok(result)
  }
}
