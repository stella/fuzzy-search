use std::collections::HashMap;
use std::panic;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use unicode_normalization::char::decompose_canonical;

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

/// Options for constructing a `FuzzySearch`.
#[napi(object)]
pub struct Options {
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
// Uses Unicode `char::is_alphanumeric()` which
// covers all scripts. CJK exception: CJK
// ideographs are treated as standalone words
// (no inter-word spaces in CJK).

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

/// Check if a match at [start..end) in a char
/// slice is at word boundaries.
fn is_whole_word_chars(
  chars: &[char],
  start: usize,
  end: usize,
) -> bool {
  let start_ok = start == 0
    || !is_word_char(chars[start - 1])
    || is_cjk(chars[start]);
  let end_ok = end >= chars.len()
    || !is_word_char(chars[end])
    || (end > 0 && is_cjk(chars[end - 1]));
  start_ok && end_ok
}

// ─── Combining mark detection ────────────────
//
// After NFD decomposition, combining marks are
// stripped to normalize diacritics. Covers all
// major combining mark Unicode blocks.

fn is_combining(c: char) -> bool {
  let cp = u32::from(c);
  // Combining Diacritical Marks
  (0x0300..=0x036F).contains(&cp)
  // Combining Diacritical Marks Extended
  || (0x1AB0..=0x1AFF).contains(&cp)
  // Combining Diacritical Marks Supplement
  || (0x1DC0..=0x1DFF).contains(&cp)
  // Combining Diacritical Marks for Symbols
  || (0x20D0..=0x20FF).contains(&cp)
  // Combining Half Marks
  || (0xFE20..=0xFE2F).contains(&cp)
  // Cyrillic combining marks
  || (0x0483..=0x0489).contains(&cp)
  // Hebrew points + marks
  || (0x0591..=0x05BD).contains(&cp)
  || cp == 0x05BF
  || (0x05C1..=0x05C2).contains(&cp)
  || (0x05C4..=0x05C5).contains(&cp)
  || cp == 0x05C7
  // Arabic combining marks
  || (0x0610..=0x061A).contains(&cp)
  || (0x064B..=0x065F).contains(&cp)
  || cp == 0x0670
  || (0x06D6..=0x06DC).contains(&cp)
  || (0x06DF..=0x06E4).contains(&cp)
  || (0x06E7..=0x06E8).contains(&cp)
  || (0x06EA..=0x06ED).contains(&cp)
}

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
        if !is_combining(dc) {
          if case_insensitive {
            for lc in dc.to_lowercase() {
              norm.push(lc);
              map.push(orig_idx);
            }
          } else {
            norm.push(dc);
            map.push(orig_idx);
          }
        }
      });
    } else {
      // case_insensitive only
      for lc in ch.to_lowercase() {
        norm.push(lc);
        map.push(orig_idx);
      }
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

// ─── Levenshtein distance ────────────────────

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
) -> Option<(usize, u8)> {
  let m = pattern.len();
  let k = dist as usize;

  let min_len = m.saturating_sub(k);
  let max_len = (m + k).min(end);

  // Try exact pattern length first (most common).
  if end >= m {
    let start = end - m;
    let d = levenshtein(pattern, &text[start..end]);
    if d <= k {
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
    let d = levenshtein(pattern, &text[start..end]);
    if d <= k {
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
) -> Vec<(usize, usize, u8)> {
  if end_positions.is_empty() {
    return vec![];
  }

  // Find local minima: positions where distance
  // is <= both neighbors (or at boundary).
  let n = end_positions.len();
  let mut minima: Vec<(usize, u8)> = Vec::new();

  for i in 0..n {
    let (pos, dist) = end_positions[i];
    let prev_dist = if i > 0 {
      end_positions[i - 1].1
    } else {
      u8::MAX
    };
    let next_dist = if i + 1 < n {
      end_positions[i + 1].1
    } else {
      u8::MAX
    };

    // Local minimum: <= prev AND < next,
    // OR at the last position of a plateau
    // (== next_dist, but next+1 is higher).
    if dist <= prev_dist && dist < next_dist {
      minima.push((pos, dist));
    } else if dist < prev_dist && dist == next_dist {
      // Start of a plateau — skip, let the
      // end of the plateau be chosen.
    } else if dist <= prev_dist
      && dist == next_dist
      && i + 1 < n
    {
      // Mid-plateau — skip.
    } else if i == n - 1 && dist <= prev_dist {
      // Last position and it's a minimum.
      if minima.last().is_none_or(|m| m.0 != pos) {
        minima.push((pos, dist));
      }
    }
  }

  // Compute start positions and collect
  // non-overlapping matches.
  let mut matches = Vec::new();
  let mut last_end: usize = 0;

  for (end, dist) in minima {
    if let Some((start, actual_dist)) =
      find_start(pattern, text, end, dist)
    {
      if start >= last_end {
        matches.push((start, end, actual_dist));
        last_end = end;
      }
    }
  }

  matches
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
  pattern_count: u32,
}

fn default_options() -> Options {
  Options {
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
    let pattern_count = patterns.len() as u32;

    let mut infos = Vec::with_capacity(patterns.len());

    for p in patterns {
      let dist = p.distance.unwrap_or(1);
      if dist > 3 {
        return Err(Error::from_reason(
          "Distance > 3 is not supported \
           (state explosion, too many \
           false positives)"
            .to_string(),
        ));
      }
      let (chars, _) = normalize_with_map(
        &p.pattern,
        normalize,
        case_insensitive,
      );
      if chars.len() > 64 {
        return Err(Error::from_reason(
          "Pattern too long (max 64 chars)".to_string(),
        ));
      }
      if chars.is_empty() {
        return Err(Error::from_reason(
          "Empty pattern".to_string(),
        ));
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
      pattern_count,
    })
  }

  /// Number of patterns in the matcher.
  #[napi(getter)]
  pub fn pattern_count(&self) -> u32 {
    self.pattern_count
  }

  /// Returns `true` if any pattern matches
  /// within its edit distance.
  #[napi]
  pub fn is_match(&self, haystack: String) -> bool {
    let orig_chars: Vec<char> = haystack.chars().collect();
    let (text_chars, pos_map) = normalize_with_map(
      &haystack,
      self.normalize_diacritics,
      self.case_insensitive,
    );

    for pat in &self.patterns {
      let ends = myers_find_ends(
        &pat.chars,
        &text_chars,
        pat.max_dist,
      );
      let matches =
        extract_matches(&pat.chars, &text_chars, &ends);
      for (start, end, _) in matches {
        if !self.whole_words {
          return true;
        }
        let orig_start = pos_map[start];
        let orig_end = pos_map[end];
        if is_whole_word_chars(
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
    let (text_chars, pos_map) = normalize_with_map(
      &haystack,
      self.normalize_diacritics,
      self.case_insensitive,
    );

    let mut all: Vec<(u32, u32, u32, u32)> = Vec::new();

    for (idx, pat) in self.patterns.iter().enumerate() {
      let ends = myers_find_ends(
        &pat.chars,
        &text_chars,
        pat.max_dist,
      );
      let matches =
        extract_matches(&pat.chars, &text_chars, &ends);

      for (start, end, dist) in matches {
        let orig_start = pos_map[start];
        let orig_end = pos_map[end];

        if self.whole_words
          && !is_whole_word_chars(
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

    // Sort by start position, then distance.
    all.sort_unstable_by(|a, b| {
      a.1.cmp(&b.1).then(a.3.cmp(&b.3))
    });

    let mut packed = Vec::with_capacity(all.len() * 4);
    for (pat, start, end, dist) in all {
      packed.push(pat);
      packed.push(start);
      packed.push(end);
      packed.push(dist);
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
    let (text_chars, pos_map) = normalize_with_map(
      &haystack,
      self.normalize_diacritics,
      self.case_insensitive,
    );

    // Collect all matches across patterns.
    let mut all: Vec<(usize, usize, u32)> = Vec::new();

    for (idx, pat) in self.patterns.iter().enumerate() {
      let ends = myers_find_ends(
        &pat.chars,
        &text_chars,
        pat.max_dist,
      );
      let matches =
        extract_matches(&pat.chars, &text_chars, &ends);

      for (start, end, _) in matches {
        let orig_start = pos_map[start];
        let orig_end = pos_map[end];

        if self.whole_words
          && !is_whole_word_chars(
            &orig_chars,
            orig_start,
            orig_end,
          )
        {
          continue;
        }
        all.push((orig_start, orig_end, idx as u32));
      }
    }

    // Sort by start, then longest match first.
    all.sort_unstable_by(|a, b| {
      a.0.cmp(&b.0).then(b.1.cmp(&a.1))
    });

    // Build result, replacing non-overlapping
    // matches.
    let mut result = String::with_capacity(haystack.len());
    let mut pos: usize = 0;

    for (start, end, pat_idx) in &all {
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
