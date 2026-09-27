use std::collections::HashMap;
use std::fmt;

use unicode_normalization::char::decompose_canonical;
use unicode_normalization::char::is_combining_mark;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Error {
  reason: String,
}

impl Error {
  #[must_use]
  pub fn from_reason(reason: impl Into<String>) -> Self {
    Self {
      reason: reason.into(),
    }
  }

  #[must_use]
  pub fn reason(&self) -> &str {
    &self.reason
  }
}

impl fmt::Display for Error {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.reason)
  }
}

impl std::error::Error for Error {}

/// Unicode Simple Case Fold (CaseFolding.txt S/C)
/// plus Turkic İ→i. Always 1:1 character mapping.
/// See `@stll/aho-corasick` for detailed rationale.
#[inline]
fn simple_case_fold(ch: char) -> char {
  match ch {
    '\u{0130}' => 'i', // İ → i (Turkic, not in S/C)
    _ => unicode_case_mapping::case_folded(ch)
      .and_then(|n| {
        let c = char::from_u32(n.get());
        debug_assert!(c.is_some(), "case_folded returned invalid code point");
        c
      })
      .unwrap_or(ch),
  }
}

fn offset_error() -> Error {
  Error::from_reason(String::from("Computed match offset is out of bounds"))
}

fn u32_overflow_error() -> Error {
  Error::from_reason(String::from("Result offset exceeds u32 range"))
}

fn get_position(map: &[usize], index: usize) -> Result<usize> {
  map.get(index).copied().ok_or_else(offset_error)
}

/// Maps a normalized exclusive end (char index) to an exclusive original-char
/// end.
///
/// Normalization is not 1:1: NFD expansion makes several normalized chars share
/// one original index, and stripped combining marks make some original chars
/// produce no normalized char at all. The correct exclusive end is the next
/// *surviving* original boundary strictly after the last original char the
/// match covers (`map[end - 1]`) — i.e. the next original char that has its own
/// normalized form, or the sentinel `orig_len`.
///
/// Walking forward to that boundary in one step: `map` is the sorted list of
/// surviving original indices (plus the sentinel), so `partition_point` finds
/// the first entry strictly greater than `map[end - 1]`. This skips both the
/// remainder of an NFD expansion (entries still equal to the last index) and
/// any stripped combining marks that follow it (which have no entry), so the
/// span always ends on a real boundary and `replace_all` never orphans a mark.
fn original_end_position(map: &[usize], end: usize) -> Result<usize> {
  let Some(last_index) = end.checked_sub(1) else {
    return get_position(map, end);
  };
  let last = get_position(map, last_index)?;
  let next = map.partition_point(|&orig| orig <= last);
  get_position(map, next)
}

/// Bounds-checked lookup into a char-index → offset map (UTF-16 or byte).
fn get_offset(map: &[u32], index: usize) -> Result<u32> {
  map.get(index).copied().ok_or_else(offset_error)
}

fn usize_to_u32(value: usize) -> Result<u32> {
  u32::try_from(value).map_err(|_| u32_overflow_error())
}

fn usize_to_u32_saturating(value: usize) -> u32 {
  u32::try_from(value).unwrap_or(u32::MAX)
}

fn distance_to_u8(distance: usize) -> Option<u8> {
  u8::try_from(distance).ok()
}

fn expanded_damerau_distance(max_dist: u8, pattern_len: usize) -> u8 {
  let doubled = usize::from(max_dist).saturating_mul(2);
  let bounded = doubled.min(pattern_len.saturating_sub(1));
  u8::try_from(bounded).unwrap_or(u8::MAX)
}

// ─── Public types ────────────────────────────

/// A pattern entry for fuzzy search.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PatternEntry {
  /// The pattern string to search for.
  pub pattern: String,
  /// Maximum edit distance (1-3). Default: 1.
  pub distance: Option<u8>,
}

/// Distance metric for fuzzy matching.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum Metric {
  /// Standard Levenshtein: insertions, deletions,
  /// substitutions.
  #[default]
  Levenshtein,
  /// Damerau-Levenshtein: insertions, deletions,
  /// substitutions, and transpositions of
  /// adjacent characters.
  DamerauLevenshtein,
}

/// Options for constructing a `FuzzySearch`.
#[derive(bon::Builder, Debug, Clone, Copy, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Options {
  /// Distance metric.
  #[builder(default = Metric::Levenshtein)]
  pub metric: Metric,
  /// Strip diacritics before matching (NFD
  /// decompose + remove combining marks).
  #[builder(default)]
  pub normalize_diacritics: bool,
  /// Use Unicode word boundaries.
  #[builder(default = true)]
  pub unicode_boundaries: bool,
  /// Only match whole words. Default: `true`.
  #[builder(default = true)]
  pub whole_words: bool,
  /// Case-insensitive matching. Default: `false`.
  #[builder(default)]
  pub case_insensitive: bool,
}

impl Default for Options {
  fn default() -> Self {
    Self::builder().build()
  }
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
fn is_whole_word_inline(chars: &[char], start: usize, end: usize) -> bool {
  if start >= end || end > chars.len() {
    return false;
  }
  let Some(current) = chars.get(start).copied() else {
    return false;
  };
  let Some(last) = end
    .checked_sub(1)
    .and_then(|index| chars.get(index))
    .copied()
  else {
    return false;
  };
  let end_ok = chars
    .get(end)
    .is_none_or(|next| !is_word_char(*next) || is_cjk(last));
  let Some(previous_index) = start.checked_sub(1) else {
    return end_ok;
  };
  let Some(previous) = chars.get(previous_index).copied() else {
    return false;
  };
  let start_ok = !is_word_char(previous) || is_cjk(current);
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
  const BITS_PER_WORD: usize = 64;

  fn new(len: usize) -> Self {
    Self {
      bits: vec![0u64; len.div_ceil(Self::BITS_PER_WORD)],
    }
  }

  #[allow(clippy::arithmetic_side_effects, clippy::integer_division)]
  fn set(&mut self, pos: usize) {
    let word = pos / Self::BITS_PER_WORD;
    let bit = pos % Self::BITS_PER_WORD;
    if let Some(slot) = self.bits.get_mut(word) {
      *slot |= 1u64 << bit;
    }
  }

  #[allow(clippy::arithmetic_side_effects, clippy::integer_division)]
  fn contains(&self, pos: usize) -> bool {
    let word = pos / Self::BITS_PER_WORD;
    let bit = pos % Self::BITS_PER_WORD;
    self
      .bits
      .get(word)
      .is_some_and(|slot| slot & (1u64 << bit) != 0)
  }
}

/// Compute UAX#29 word boundaries as char-index
/// positions (not byte offsets).
fn compute_char_boundaries(text: &str) -> CharBoundarySet {
  use unicode_segmentation::UnicodeSegmentation;
  // Build byte-offset → char-index map.
  let mut byte_to_char: Vec<usize> =
    Vec::with_capacity(text.len().saturating_add(1));
  let mut char_idx = 0;
  for ch in text.chars() {
    for _ in 0..ch.len_utf8() {
      byte_to_char.push(char_idx);
    }
    char_idx = char_idx.saturating_add(1);
  }
  byte_to_char.push(char_idx); // sentinel

  let mut bs = CharBoundarySet::new(char_idx.saturating_add(1));
  bs.set(0);
  bs.set(char_idx);
  for (byte_off, word) in text.unicode_word_indices() {
    let Some(end_byte) = byte_off.checked_add(word.len()) else {
      continue;
    };
    if let (Some(&start_char), Some(&end_char)) =
      (byte_to_char.get(byte_off), byte_to_char.get(end_byte))
    {
      bs.set(start_char);
      bs.set(end_char);
    }
  }
  bs
}

/// Boundary mode: inline or UAX#29 segmenter.
enum BoundaryMode {
  Inline,
  Segmenter { bitset: CharBoundarySet },
}

impl BoundaryMode {
  fn is_whole_word(&self, chars: &[char], start: usize, end: usize) -> bool {
    match self {
      Self::Inline => is_whole_word_inline(chars, start, end),
      Self::Segmenter { bitset } => {
        if start >= end || end > chars.len() {
          return false;
        }
        bitset.contains(start) && bitset.contains(end)
      }
    }
  }
}

/// Choose boundary mode based on text content.
fn choose_boundary_mode(text: &str, unicode_boundaries: bool) -> BoundaryMode {
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
  let mut map = Vec::with_capacity(orig_len.saturating_add(1));

  for (orig_idx, &ch) in orig_chars.iter().enumerate() {
    if strip_dia {
      decompose_canonical(ch, |dc| {
        if !is_combining_mark(dc) {
          let normalized = if case_insensitive {
            simple_case_fold(dc)
          } else {
            dc
          };
          norm.push(normalized);
          map.push(orig_idx);
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

#[allow(clippy::arithmetic_side_effects)]
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

  let mask = if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
  let msb = 1u64 << (m - 1);

  // PV = positive vertical deltas (all +1 init)
  // MV = negative vertical deltas (all 0 init)
  let mut pv: u64 = mask;
  let mut mv: u64 = 0;
  let Ok(mut score) = i32::try_from(m) else {
    return vec![];
  };

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
      let Some(end) = j.checked_add(1) else {
        continue;
      };
      if let Ok(distance) = u8::try_from(score) {
        results.push((end, distance));
      }
    }
  }

  results
}

// ─── Distance functions ─────────────────────

/// Standard Levenshtein edit distance on char
/// slices. O(m × n) time, O(n) space.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
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
      curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
    }
    prev = curr;
  }
  prev[n]
}

/// Optimal String Alignment (restricted Damerau-
/// Levenshtein) on char slices. Counts adjacent
/// transpositions as a single edit.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
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
      curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
      // Transposition: if a[i-1]==b[j-2] and
      // a[i-2]==b[j-1], swapping is 1 edit.
      if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
        curr[j] = curr[j].min(prev2[j - 2] + 1);
      }
    }
    prev2 = prev;
    prev = curr;
  }
  prev[n]
}

/// Dispatch to the correct distance function.
fn edit_distance(a: &[char], b: &[char], use_damerau: bool) -> usize {
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

/// Normalized haystack plus what is needed to map
/// spans back to the original text.
struct Haystack {
  orig_chars: Vec<char>,
  text_chars: Vec<char>,
  pos_map: Vec<usize>,
  boundary: BoundaryMode,
}

impl Haystack {
  fn new(haystack: &str, search: &FuzzySearch) -> Self {
    let (text_chars, pos_map) = normalize_with_map(
      haystack,
      search.normalize_diacritics,
      search.case_insensitive,
    );
    Self {
      orig_chars: haystack.chars().collect(),
      text_chars,
      pos_map,
      boundary: choose_boundary_mode(haystack, search.unicode_boundaries),
    }
  }

  /// Maps a normalized `[start, end)` span to
  /// original char indices.
  fn original_span(&self, start: usize, end: usize) -> Result<(usize, usize)> {
    Ok((
      get_position(&self.pos_map, start)?,
      original_end_position(&self.pos_map, end)?,
    ))
  }
}

/// Which windows `find_start` may return.
#[derive(Clone, Copy)]
enum Alignment<'a> {
  /// Any window within the distance budget.
  Any,
  /// Only windows spanning whole words of the
  /// original text.
  WholeWord(&'a Haystack),
}

impl Alignment<'_> {
  fn accepts(self, start: usize, end: usize) -> Result<bool> {
    match self {
      Self::Any => Ok(true),
      Self::WholeWord(haystack) => {
        let (orig_start, orig_end) = haystack.original_span(start, end)?;
        Ok(haystack.boundary.is_whole_word(
          &haystack.orig_chars,
          orig_start,
          orig_end,
        ))
      }
    }
  }
}

fn find_start(
  pattern: &[char],
  text: &[char],
  end: usize,
  dist: u8,
  actual_max: u8,
  use_damerau: bool,
  alignment: Alignment<'_>,
) -> Result<Option<(usize, u8)>> {
  let m = pattern.len();
  // `dist` determines the window range (from
  // Myers prefilter). `actual_max` is the real
  // distance threshold for the chosen metric.
  // A whole-word window may be worse than the
  // best window ending here, so its length can
  // differ from `m` by up to `actual_max`.
  let k = match alignment {
    Alignment::Any => usize::from(dist),
    Alignment::WholeWord(_) => usize::from(dist.max(actual_max)),
  };
  let max_k = usize::from(actual_max);

  // Enforce min_len >= 1 to avoid zero-length
  // matches (e.g. pattern "ab" dist 2 matching "").
  let min_len = m.saturating_sub(k).max(1);
  let max_len = m.saturating_add(k).min(end);

  // Try exact pattern length first (most common).
  if let Some(start) = end.checked_sub(m)
    && let Some(window) = text.get(start..end)
    && alignment.accepts(start, end)?
  {
    let d = edit_distance(pattern, window, use_damerau);
    if d <= max_k {
      return Ok(distance_to_u8(d).map(|distance| (start, distance)));
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
    let Some(start) = end.checked_sub(len) else {
      continue;
    };
    let Some(window) = text.get(start..end) else {
      continue;
    };
    if !alignment.accepts(start, end)? {
      continue;
    }
    let d = edit_distance(pattern, window, use_damerau);
    if d <= max_k {
      let Some(distance) = distance_to_u8(d) else {
        continue;
      };
      match best {
        None => best = Some((start, distance)),
        Some((_, bd)) if distance < bd => {
          best = Some((start, distance));
        }
        _ => {}
      }
    }
  }
  Ok(best)
}

// ─── Match region extraction ─────────────────
//
// From Myers end positions, extract local minima
// in the distance curve and compute start
// positions. Returns non-overlapping matches
// sorted by start position.

#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn extract_matches(
  pattern: &[char],
  text: &[char],
  end_positions: &[(usize, u8)],
  window_dist: u8,
  actual_max: u8,
  use_damerau: bool,
  alignment: Alignment<'_>,
) -> Result<Vec<(usize, usize, u8)>> {
  if end_positions.is_empty() {
    return Ok(vec![]);
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
    let k = usize::from(window_dist);
    let window_bound = end + 2 * m + k;
    let mut best: Option<(usize, usize, u8)> = None;
    let mut best_end_idx = i;
    let mut j = i;
    while j < end_positions.len()
      && end_positions[j].0 <= window_bound
      && (j == i || end_positions[j].0 == end_positions[j - 1].0 + 1)
    {
      let (je, jd) = end_positions[j];
      if let Some((start, actual_dist)) =
        find_start(pattern, text, je, jd, actual_max, use_damerau, alignment)?
        && start >= last_match_end
      {
        let len = je - start;
        let len_diff = len.abs_diff(m);
        let is_better = match best {
          None => true,
          Some((bs, be, bd)) => {
            let bl = be - bs;
            let bl_diff = bl.abs_diff(m);
            actual_dist < bd
              || (actual_dist == bd && len_diff < bl_diff)
              || (actual_dist == bd && len_diff == bl_diff && start < bs)
          }
        };
        if is_better {
          best = Some((start, je, actual_dist));
          best_end_idx = j;
        }
      }
      j += 1;
    }

    if let Some((start, be, dist)) = best {
      matches.push((start, be, dist));
      last_match_end = be;
      // Skip past this match.
      i = best_end_idx + 1;
      while i < end_positions.len() && end_positions[i].0 <= be {
        i += 1;
      }
    } else {
      i += 1;
    }
  }

  Ok(matches)
}

// ─── Standalone distance function ────────────

/// Compute edit distance between two strings.
/// Uses Unicode characters (not UTF-16 code
/// units), so emoji and supplementary plane
/// characters are handled correctly.
///
/// `metric` chooses Levenshtein or Damerau-
/// Levenshtein transposition handling.
#[must_use]
pub fn distance(a: &str, b: &str, metric: Metric) -> u32 {
  let ac: Vec<char> = a.chars().collect();
  let bc: Vec<char> = b.chars().collect();
  let use_damerau = metric == Metric::DamerauLevenshtein;
  usize_to_u32_saturating(edit_distance(&ac, &bc, use_damerau))
}

// ─── UTF-16 offset mapping ──────────────────

/// Build a char-index → UTF-16 code unit offset
/// mapping. Index `i` gives the UTF-16 offset of
/// char `i`; index `len` is the total length.
fn build_utf16_map(chars: &[char]) -> Result<Vec<u32>> {
  let mut map = Vec::with_capacity(chars.len().saturating_add(1));
  let mut utf16_pos: u32 = 0;
  for &ch in chars {
    map.push(utf16_pos);
    let width = usize_to_u32(ch.len_utf16())?;
    utf16_pos = utf16_pos
      .checked_add(width)
      .ok_or_else(u32_overflow_error)?;
  }
  map.push(utf16_pos);
  Ok(map)
}

/// Build a char-index → UTF-8 byte offset mapping.
/// Index `i` gives the byte offset of char `i`;
/// index `len` is the total byte length.
///
/// Scans raw bytes: every byte that is not a UTF-8 continuation byte
/// (`0b10xxxxxx`) starts a new character, so boundaries fall out of one linear
/// pass with no decoding.
fn build_byte_map(haystack: &str, char_count: usize) -> Result<Vec<u32>> {
  let mut map = Vec::with_capacity(char_count.saturating_add(1));
  for (index, &byte) in haystack.as_bytes().iter().enumerate() {
    if (byte & 0xC0) != 0x80 {
      map.push(usize_to_u32(index)?);
    }
  }
  map.push(usize_to_u32(haystack.len())?);
  Ok(map)
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
#[allow(clippy::struct_excessive_bools)]
pub struct FuzzySearch {
  patterns: Vec<PatternInfo>,
  normalize_diacritics: bool,
  case_insensitive: bool,
  whole_words: bool,
  unicode_boundaries: bool,
  use_damerau: bool,
  pattern_count: u32,
}

impl FuzzySearch {
  /// Build a fuzzy matcher from the given
  /// patterns and options.
  pub fn new(patterns: Vec<PatternEntry>, options: Options) -> Result<Self> {
    let normalize = options.normalize_diacritics;
    let case_insensitive = options.case_insensitive;
    let whole_words = options.whole_words;
    let unicode_boundaries = options.unicode_boundaries;
    let use_damerau = options.metric == Metric::DamerauLevenshtein;
    let pattern_count = usize_to_u32(patterns.len())?;

    let mut infos = Vec::with_capacity(patterns.len());

    for p in patterns {
      let dist = p.distance.unwrap_or(1);
      // Myers is O(n) regardless of distance,
      // so no hard upper limit. But distance >=
      // pattern length means nearly everything
      // matches (useless noise).
      let (chars, _) =
        normalize_with_map(&p.pattern, normalize, case_insensitive);
      if chars.is_empty() {
        return Err(Error::from_reason("Empty pattern".to_string()));
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
      if usize::from(dist) >= chars.len() {
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
  #[must_use]
  pub const fn pattern_count(&self) -> u32 {
    self.pattern_count
  }

  /// Find end positions. For Damerau, run Myers
  /// with expanded distance (2k) as a prefilter
  /// since Levenshtein(a,b) <= 2 * Damerau(a,b).
  /// The actual Damerau distance is computed in
  /// `find_start` during verification.
  fn find_ends(
    &self,
    pattern: &[char],
    text: &[char],
    max_dist: u8,
  ) -> Vec<(usize, u8)> {
    if self.use_damerau {
      // Conservative prefilter: any Damerau-k
      // match has Levenshtein distance <= 2k.
      let prefilter_dist = expanded_damerau_distance(max_dist, pattern.len());
      myers_find_ends(pattern, text, prefilter_dist)
    } else {
      myers_find_ends(pattern, text, max_dist)
    }
  }

  /// Matches of one pattern as original-char
  /// `(start, end, distance)` spans. With
  /// `whole_words`, only whole-word windows are
  /// candidates, so a non-whole-word window never
  /// displaces an overlapping whole-word one.
  fn pattern_matches(
    &self,
    pat: &PatternInfo,
    haystack: &Haystack,
  ) -> Result<Vec<(usize, usize, u8)>> {
    let pattern = pat.chars.as_slice();
    let max_dist = pat.max_dist;
    let ends = self.find_ends(pattern, &haystack.text_chars, max_dist);
    // For Damerau: use expanded window for
    // candidate search, but filter by actual
    // max_dist via the distance function.
    let window_dist = if self.use_damerau {
      expanded_damerau_distance(max_dist, pattern.len())
    } else {
      max_dist
    };
    let alignment = if self.whole_words {
      Alignment::WholeWord(haystack)
    } else {
      Alignment::Any
    };
    let matches = extract_matches(
      pattern,
      &haystack.text_chars,
      &ends,
      window_dist,
      max_dist,
      self.use_damerau,
      alignment,
    )?;
    matches
      .into_iter()
      .map(|(start, end, dist)| {
        let (orig_start, orig_end) = haystack.original_span(start, end)?;
        Ok((orig_start, orig_end, dist))
      })
      .collect()
  }

  /// Returns `true` if any pattern matches
  /// within its edit distance.
  pub fn is_match(&self, haystack: &str) -> Result<bool> {
    let prepared = Haystack::new(haystack, self);
    for pat in &self.patterns {
      if !self.pattern_matches(pat, &prepared)?.is_empty() {
        return Ok(true);
      }
    }
    Ok(false)
  }

  /// Returns packed `[pattern, start, end,
  /// distance]` quads using UTF-16 offsets.
  pub fn find_iter_packed(&self, haystack: &str) -> Result<Vec<u32>> {
    let prepared = Haystack::new(haystack, self);
    let utf16_map = build_utf16_map(&prepared.orig_chars)?;

    let mut all: Vec<(u32, u32, u32, u32)> = Vec::new();

    for (idx, pat) in self.patterns.iter().enumerate() {
      for (orig_start, orig_end, dist) in
        self.pattern_matches(pat, &prepared)?
      {
        let utf16_start = get_offset(&utf16_map, orig_start)?;
        let utf16_end = get_offset(&utf16_map, orig_end)?;
        all.push((usize_to_u32(idx)?, utf16_start, utf16_end, u32::from(dist)));
      }
    }

    // Sort by start position, then distance
    // (prefer lower), then longer match.
    all.sort_unstable_by(|a, b| {
      a.1.cmp(&b.1).then(a.3.cmp(&b.3)).then(b.2.cmp(&a.2))
    });

    // Greedy non-overlapping across all patterns.
    let mut packed = Vec::with_capacity(all.len().saturating_mul(4));
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

    Ok(packed)
  }

  /// Like [`find_iter_packed`](Self::find_iter_packed) but emits UTF-8 byte
  /// offsets instead of UTF-16 code-unit offsets.
  ///
  /// The matcher works on chars and already has positions in hand; this maps
  /// them to byte offsets directly, skipping the UTF-16 conversion
  /// `find_iter_packed` performs. It is the native unit for Rust consumers that
  /// slice `&str` directly; UTF-16 consumers (e.g. JavaScript) keep using
  /// [`find_iter_packed`](Self::find_iter_packed).
  ///
  /// The packed layout is identical: `[pattern, start, end, distance]` quads.
  /// Only `start`/`end` change unit (UTF-8 bytes); `pattern` and `distance` are
  /// preserved exactly.
  pub fn find_iter_packed_bytes(&self, haystack: &str) -> Result<Vec<u32>> {
    let prepared = Haystack::new(haystack, self);
    let byte_map = build_byte_map(haystack, prepared.orig_chars.len())?;

    let mut all: Vec<(u32, u32, u32, u32)> = Vec::new();

    for (idx, pat) in self.patterns.iter().enumerate() {
      for (orig_start, orig_end, dist) in
        self.pattern_matches(pat, &prepared)?
      {
        let byte_start = get_offset(&byte_map, orig_start)?;
        let byte_end = get_offset(&byte_map, orig_end)?;
        all.push((usize_to_u32(idx)?, byte_start, byte_end, u32::from(dist)));
      }
    }

    // Sort by start position, then distance
    // (prefer lower), then longer match.
    all.sort_unstable_by(|a, b| {
      a.1.cmp(&b.1).then(a.3.cmp(&b.3)).then(b.2.cmp(&a.2))
    });

    // Greedy non-overlapping across all patterns.
    let mut packed = Vec::with_capacity(all.len().saturating_mul(4));
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

    Ok(packed)
  }

  /// Replace all fuzzy matches.
  /// `replacements[i]` replaces pattern `i`.
  pub fn replace_all(
    &self,
    haystack: &str,
    replacements: &[String],
  ) -> Result<String> {
    let expected_replacements =
      usize::try_from(self.pattern_count).map_err(|_| u32_overflow_error())?;
    if replacements.len() != expected_replacements {
      return Err(Error::from_reason(format!(
        "Expected {} replacements, got {}",
        self.pattern_count,
        replacements.len()
      )));
    }

    let prepared = Haystack::new(haystack, self);
    let orig_chars = &prepared.orig_chars;

    // Collect all matches across patterns.
    // (start, end, pat_idx, distance)
    let mut all: Vec<(usize, usize, u32, u8)> = Vec::new();

    for (idx, pat) in self.patterns.iter().enumerate() {
      for (orig_start, orig_end, dist) in
        self.pattern_matches(pat, &prepared)?
      {
        all.push((orig_start, orig_end, usize_to_u32(idx)?, dist));
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
      let Some(prefix) = orig_chars.get(pos..*start) else {
        return Err(offset_error());
      };
      for &ch in prefix {
        result.push(ch);
      }
      let replacement_index =
        usize::try_from(*pat_idx).map_err(|_| u32_overflow_error())?;
      let Some(replacement) = replacements.get(replacement_index) else {
        return Err(Error::from_reason(String::from(
          "Replacement index is out of bounds",
        )));
      };
      result.push_str(replacement);
      pos = *end;
    }
    let Some(suffix) = orig_chars.get(pos..) else {
      return Err(offset_error());
    };
    for &ch in suffix {
      result.push(ch);
    }

    Ok(result)
  }
}

#[cfg(test)]
#[allow(
  clippy::unwrap_used,
  clippy::missing_assert_message,
  clippy::arithmetic_side_effects,
  clippy::cast_possible_truncation,
  clippy::as_conversions,
  clippy::indexing_slicing
)]
mod tests {
  use super::{
    FuzzySearch, Metric, Options, PatternEntry, edit_distance,
    is_whole_word_inline, normalize_with_map, original_end_position,
  };

  fn matcher(pattern: &str) -> FuzzySearch {
    FuzzySearch::new(
      vec![PatternEntry {
        pattern: String::from(pattern),
        distance: Some(1),
      }],
      Options::default(),
    )
    .unwrap()
  }

  #[test]
  fn packed_bytes_emit_byte_offsets() {
    // `ä` is 2 UTF-8 bytes but 1 UTF-16 code unit, so the offset units
    // diverge. Haystack chars: ä(0) ` `(1) c(2) a(3) t(4), end at 5.
    //   UTF-16: ä=1 unit, so `cat` spans units [2, 5).
    //   bytes:  ä=2 bytes, so `cat` spans bytes [3, 6).
    let ac = matcher("cat");
    let haystack = "ä cat";

    // Existing packed output is UTF-16: [pattern, start, end, distance].
    // Distance is 0 (exact match) and must be preserved in both variants.
    assert_eq!(ac.find_iter_packed(haystack).unwrap(), vec![0, 2, 5, 0]);

    // Byte variant reports byte offsets but keeps pattern and distance.
    let packed = ac.find_iter_packed_bytes(haystack).unwrap();
    assert_eq!(packed, vec![0, 3, 6, 0]);

    // The byte offsets index the original `&str` slice as the matched text.
    let start = usize::try_from(*packed.get(1).unwrap()).unwrap();
    let end = usize::try_from(*packed.get(2).unwrap()).unwrap();
    assert_eq!(haystack.get(start..end), Some("cat"));
  }

  #[test]
  fn normalized_match_end_rounds_to_original_char_boundary() {
    // Regression: NFD-decomposing `각` (U+AC01) yields three jamo that all map
    // back to the single original char. A fuzzy match on the `가` (U+AC00 → two
    // jamo) prefix ends inside that expansion, so a naive `pos_map[end]`
    // collapses start == end and returns an empty span. The original-char end
    // must round up to cover the whole syllable. Affects both the UTF-16 and
    // byte packed paths, which share the offset derivation.
    let search = FuzzySearch::new(
      vec![PatternEntry {
        pattern: String::from("가"),
        distance: Some(1),
      }],
      Options {
        normalize_diacritics: true,
        whole_words: false,
        ..Options::default()
      },
    )
    .unwrap();

    let haystack = "각";
    let utf16 = search.find_iter_packed(haystack).unwrap();
    let bytes = search.find_iter_packed_bytes(haystack).unwrap();

    // Exactly one match, with a non-empty span in both unit systems.
    assert_eq!(utf16.len(), 4, "expected one packed match (4 fields)");
    assert_eq!(bytes.len(), 4, "expected one packed match (4 fields)");

    let u_start = *utf16.get(1).unwrap();
    let u_end = *utf16.get(2).unwrap();
    assert!(u_end > u_start, "UTF-16 span must be non-empty");

    let b_start = *bytes.get(1).unwrap();
    let b_end = *bytes.get(2).unwrap();
    assert!(b_end > b_start, "byte span must be non-empty");

    // The byte span slices back to the whole matched syllable.
    let start = usize::try_from(b_start).unwrap();
    let end = usize::try_from(b_end).unwrap();
    assert_eq!(haystack.get(start..end), Some("각"));
  }

  #[test]
  fn normalized_match_end_keeps_trailing_stripped_combining_marks() {
    // Inverse of the NFD-expansion case: `a\u{0301}` (a + combining acute)
    // normalizes to `a` because the mark is stripped, so multiple original
    // chars collapse to one normalized char. A match on the base must still
    // cover the stripped mark, otherwise `replace_all` leaves the accent
    // behind. Here `má` (m + a + ´) → normalized `ma`; matching `ma` must
    // span the original `ma\u{0301}` so the replacement is span-safe.
    let search = FuzzySearch::new(
      vec![PatternEntry {
        pattern: String::from("ma"),
        distance: Some(1),
      }],
      Options {
        normalize_diacritics: true,
        whole_words: false,
        ..Options::default()
      },
    )
    .unwrap();

    let haystack = "ma\u{0301}s";

    // The byte span for the `ma` match includes the trailing combining mark.
    let bytes = search.find_iter_packed_bytes(haystack).unwrap();
    let b_start = usize::try_from(*bytes.get(1).unwrap()).unwrap();
    let b_end = usize::try_from(*bytes.get(2).unwrap()).unwrap();
    assert_eq!(haystack.get(b_start..b_end), Some("ma\u{0301}"));

    // Replacement is span-safe: the accent is consumed, not left dangling.
    assert_eq!(
      search.replace_all(haystack, &[String::from("X")]).unwrap(),
      "Xs"
    );
  }

  #[test]
  fn normalized_match_end_covers_expansion_then_trailing_mark() {
    // Compound case: an NFD expansion (Hangul `각` → ㄱㅏㄱ) whose original char
    // is immediately followed by a stripped combining mark. A match on the `가`
    // prefix ends inside the expansion, so both single-sided rules stop too
    // early and leave the accent. The end must skip to the next surviving
    // boundary (`x`), covering the whole syllable and its trailing mark.
    let search = FuzzySearch::new(
      vec![PatternEntry {
        pattern: String::from("가"),
        distance: Some(1),
      }],
      Options {
        normalize_diacritics: true,
        whole_words: false,
        ..Options::default()
      },
    )
    .unwrap();

    let haystack = "각\u{0301}x";

    let bytes = search.find_iter_packed_bytes(haystack).unwrap();
    let b_start = usize::try_from(*bytes.get(1).unwrap()).unwrap();
    let b_end = usize::try_from(*bytes.get(2).unwrap()).unwrap();
    assert_eq!(haystack.get(b_start..b_end), Some("각\u{0301}"));

    assert_eq!(
      search.replace_all(haystack, &[String::from("X")]).unwrap(),
      "Xx"
    );
  }

  fn matcher_with(
    pattern: &str,
    distance: u8,
    options: Options,
  ) -> FuzzySearch {
    FuzzySearch::new(
      vec![PatternEntry {
        pattern: String::from(pattern),
        distance: Some(distance),
      }],
      options,
    )
    .unwrap()
  }

  #[test]
  fn whole_word_match_beside_a_shorter_overlapping_candidate() {
    let search = matcher_with("putin", 2, Options::default());

    // The exact-length window `uttin` is not whole-word; `puttin` (distance 1)
    // is.
    assert_eq!(search.find_iter_packed("puttin").unwrap(), vec![0, 0, 6, 1]);
    // The distance-0 prefix `putin` is not whole-word; `putinov` (distance 2)
    // is.
    assert_eq!(
      search.find_iter_packed("putinov").unwrap(),
      vec![0, 0, 7, 2]
    );
    assert_eq!(
      search.find_iter_packed("a putinov b").unwrap(),
      vec![0, 2, 9, 2]
    );
    // Leading insertions: the distance-0 suffix is not whole-word.
    assert_eq!(
      search.find_iter_packed("xxputin").unwrap(),
      vec![0, 0, 7, 2]
    );
    assert_eq!(
      search.find_iter_packed_bytes("é xxputin").unwrap(),
      vec![0, 3, 10, 2]
    );

    assert!(search.is_match("puttin").unwrap());
    assert!(search.is_match("putinov").unwrap());
    assert_eq!(
      search
        .replace_all("a putinov b puttin", &[String::from("X")])
        .unwrap(),
      "a X b X"
    );
    // Beyond the budget there is still no whole-word match.
    assert!(!search.is_match("putinovo").unwrap());
  }

  #[test]
  fn whole_word_match_with_normalized_diacritics() {
    let search = matcher_with(
      "putin",
      2,
      Options {
        normalize_diacritics: true,
        ..Options::default()
      },
    );

    // Precomposed and decomposed accents; offsets stay in original UTF-16
    // units and cover the trailing combining mark.
    assert_eq!(
      search.find_iter_packed("a pútinov").unwrap(),
      vec![0, 2, 9, 2]
    );
    assert_eq!(
      search.find_iter_packed("a pu\u{0301}tinov").unwrap(),
      vec![0, 2, 10, 2]
    );
    assert_eq!(
      search.find_iter_packed("puttín\u{0301}").unwrap(),
      vec![0, 0, 7, 1]
    );
    assert_eq!(
      search
        .replace_all("a pu\u{0301}tinov b", &[String::from("X")])
        .unwrap(),
      "a X b"
    );
  }

  #[test]
  fn whole_word_match_with_damerau_metric() {
    let search = matcher_with(
      "putin",
      2,
      Options {
        metric: Metric::DamerauLevenshtein,
        ..Options::default()
      },
    );
    assert_eq!(
      search.find_iter_packed("putinov").unwrap(),
      vec![0, 0, 7, 2]
    );
    assert_eq!(search.find_iter_packed("uptinx").unwrap(), vec![0, 0, 6, 2]);
  }

  #[test]
  fn substring_mode_keeps_its_candidate_selection() {
    let search = matcher_with(
      "putin",
      2,
      Options {
        whole_words: false,
        ..Options::default()
      },
    );
    assert_eq!(search.find_iter_packed("puttin").unwrap(), vec![0, 0, 5, 2]);
    assert_eq!(
      search.find_iter_packed("putinov").unwrap(),
      vec![0, 0, 5, 0]
    );
    assert_eq!(
      search.find_iter_packed("xxputin").unwrap(),
      vec![0, 2, 7, 0]
    );
    assert_eq!(
      search
        .replace_all("a putinov b", &[String::from("X")])
        .unwrap(),
      "a Xov b"
    );
  }

  /// Whole-word completeness and soundness: `is_match` is true exactly when
  /// some whole-word window of the haystack is within the distance budget.
  #[test]
  fn fuzz_whole_word_is_match_agrees_with_brute_force() {
    let alphabet: [char; 7] = ['a', 'b', 'c', 'd', ' ', '.', 'a'];
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);

    for iteration in 0..20_000 {
      let use_damerau = iteration % 2 == 1;
      let pattern_len = rng.below(4) + 2;
      let pattern: Vec<char> =
        (0..pattern_len).map(|_| alphabet[rng.below(4)]).collect();
      let max_dist = u8::try_from(rng.below(pattern_len.min(3)) + 1)
        .unwrap()
        .min(u8::try_from(pattern_len - 1).unwrap());
      let text_len = rng.below(12) + 1;
      let text: Vec<char> = (0..text_len)
        .map(|_| alphabet[rng.below(alphabet.len())])
        .collect();

      let pattern_str: String = pattern.iter().collect();
      let text_str: String = text.iter().collect();
      let search = matcher_with(
        &pattern_str,
        max_dist,
        Options {
          metric: if use_damerau {
            Metric::DamerauLevenshtein
          } else {
            Metric::Levenshtein
          },
          unicode_boundaries: false,
          ..Options::default()
        },
      );

      let expected = (0..text_len).any(|start| {
        (start + 1..=text_len).any(|end| {
          is_whole_word_inline(&text, start, end)
            && edit_distance(&pattern, &text[start..end], use_damerau)
              <= usize::from(max_dist)
        })
      });

      assert_eq!(
        search.is_match(&text_str).unwrap(),
        expected,
        "pattern={pattern_str:?} k={max_dist} damerau={use_damerau} \
         text={text_str:?}"
      );
      assert_eq!(
        !search.find_iter_packed(&text_str).unwrap().is_empty(),
        expected,
        "pattern={pattern_str:?} k={max_dist} damerau={use_damerau} \
         text={text_str:?}"
      );
    }
  }

  // Small deterministic xorshift PRNG so the fuzz test is reproducible.
  struct Rng(u64);

  impl Rng {
    fn next_u64(&mut self) -> u64 {
      let mut x = self.0;
      x ^= x << 13;
      x ^= x >> 7;
      x ^= x << 17;
      self.0 = x;
      x
    }

    fn below(&mut self, bound: usize) -> usize {
      (self.next_u64() % bound as u64) as usize
    }

    fn boolean(&mut self) -> bool {
      self.next_u64() & 1 == 1
    }
  }

  /// Property fuzz over `original_end_position`: across thousands of random
  /// original strings (base letters, combining marks, Hangul syllables, spaces)
  /// and random normalized match ranges, the derived original-char span must
  /// always land on a real boundary and never orphan a stripped combining mark.
  ///
  /// `pos_map` is the source of truth for which original chars survive
  /// normalization, so the oracle (`expected`) is computed independently of the
  /// function under test: it is the smallest surviving original index — or the
  /// sentinel `orig_len` — strictly greater than the last covered original char.
  #[test]
  fn fuzz_normalized_end_lands_on_surviving_boundary() {
    // Mix of: plain bases, combining marks (stripped), precomposed and
    // Hangul (NFD-expanding), and a space.
    let alphabet: [char; 8] =
      ['a', 'b', 'x', ' ', '\u{0301}', '\u{0302}', '각', '가'];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);

    for _ in 0..20_000 {
      let len = rng.below(8) + 1;
      let original: String = (0..len)
        .map(|_| alphabet[rng.below(alphabet.len())])
        .collect();
      let (norm, pos_map) = normalize_with_map(&original, true, rng.boolean());
      if norm.is_empty() {
        continue;
      }

      let orig_len = original.chars().count();
      // Surviving original indices = the entries of pos_map before its sentinel.
      let surviving = &pos_map[..norm.len()];
      let survives = |index: usize| surviving.binary_search(&index).is_ok();

      let start = rng.below(norm.len());
      let end = start + 1 + rng.below(norm.len() - start);

      let orig_start = *pos_map.get(start).unwrap();
      let orig_end = original_end_position(&pos_map, end).unwrap();
      let last_covered = *pos_map.get(end - 1).unwrap();

      // Independent oracle: next surviving boundary strictly after last_covered.
      let expected = (last_covered + 1..=orig_len)
        .find(|&i| i == orig_len || survives(i))
        .unwrap();

      assert_eq!(
        orig_end, expected,
        "original={original:?} start={start} end={end} map={pos_map:?}"
      );
      // Span is non-empty, in range, and covers the last matched char.
      assert!(orig_start < orig_end, "empty span for {original:?}");
      assert!(orig_end <= orig_len);
      assert!(last_covered < orig_end);
      // Never stops on a stripped combining mark (replace-safety).
      assert!(
        orig_end == orig_len || survives(orig_end),
        "orphaned mark at {orig_end} in {original:?} map={pos_map:?}"
      );
    }
  }
}
