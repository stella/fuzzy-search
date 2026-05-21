/* Shared core: types, helpers, and classes that
 * use a late-bound native backend (NAPI-RS or WASM).
 * Call initBinding() before constructing classes. */

// -- Native binding types ----------------------------

export type NativeBinding = {
  FuzzySearch: new (
    entries: NormalizedEntry[],
    options?: Options,
  ) => NativeFuzzySearch;
  distance: (
    a: string,
    b: string,
    metric: Metric | null,
  ) => number;
};

type NativeFuzzySearch = {
  patternCount: number;
  isMatch(haystack: string): boolean;
  _findIterPacked(haystack: string): Uint32Array;
  replaceAll(
    haystack: string,
    replacements: string[],
  ): string;
};

type NormalizedEntry = {
  pattern: string;
  distance?: number;
  name?: string;
};

// -- Late-bound native binding -----------------------

let binding: NativeBinding;

/** Set the native backend. Must be called once
 *  before any class constructor. */
export const initBinding = (b: NativeBinding) => {
  binding = b;
};

// -- Public types ------------------------------------

/** Distance metric for fuzzy matching. */
export type Metric = "levenshtein" | "damerau-levenshtein";

/** Options for constructing a `FuzzySearch`. */
export type Options = {
  /**
   * Distance metric.
   * - `"levenshtein"`: insertions, deletions,
   *   substitutions (default).
   * - `"damerau-levenshtein"`: + transpositions
   *   of adjacent characters (ab -> ba = 1 edit).
   * @default "levenshtein"
   */
  metric?: Metric;
  /**
   * Strip diacritics before matching (NFD
   * decompose + remove combining marks).
   * "Pribram" matches "Pribram" at distance 0.
   * @default false
   */
  normalizeDiacritics?: boolean;
  /**
   * Use Unicode word boundaries (covers all
   * scripts). CJK characters are treated as
   * standalone words.
   * @default true
   */
  unicodeBoundaries?: boolean;
  /**
   * Only match whole words. Fuzzy matches on
   * substrings are usually noise; require word
   * boundaries unless opted out.
   * @default true
   */
  wholeWords?: boolean;
  /**
   * Case-insensitive matching (Unicode-aware).
   * @default false
   */
  caseInsensitive?: boolean;
  /**
   * Drop matches whose normalized similarity
   * score is below this threshold. Score is
   * `1 - distance / pattern.length`, clamped to
   * `[0, 1]`. The comparison is inclusive:
   * `score >= minScore` keeps the match.
   *
   * Applied after distance filtering, before
   * `kBest` ranking. Does not affect
   * `replaceAll`.
   */
  minScore?: number;
  /**
   * Return only the top `k` matches across the
   * entire haystack, ranked by score descending.
   * Ties are broken by lower `start`, then by
   * pattern index ascending for deterministic
   * ordering. Returned matches are sorted by
   * score (highest first), not by `start`.
   *
   * Applied after `minScore`. Does not affect
   * `replaceAll`.
   */
  kBest?: number;
};

/** A pattern entry with its edit distance. */
export type PatternEntry =
  | string
  | {
      pattern: string;
      /** Max edit distance. Must be less than
       *  pattern length. `"auto"` uses the
       *  Elasticsearch convention: 1-2 chars -> 0,
       *  3-5 chars -> 1, 6+ chars -> 2.
       *  @default 1 */
      distance?: number | "auto";
      /** Optional name for the pattern. */
      name?: string;
    };

/** A single fuzzy match result. */
export type FuzzyMatch = {
  /** Index into the patterns array. */
  pattern: number;
  /** Start UTF-16 code unit offset (compatible
   *  with `String.prototype.slice()`). */
  start: number;
  /** End offset (exclusive). */
  end: number;
  /** The matched text
   *  (`haystack.slice(start, end)`). */
  text: string;
  /** Actual Levenshtein edit distance. */
  distance: number;
  /**
   * Normalized similarity in `[0, 1]`:
   * `1 - distance / pattern.length`, clamped at 0.
   * Always populated. `distance=0` yields `1.0`
   * (perfect); higher distances yield lower scores.
   * Lets callers rank across patterns of differing
   * lengths without computing the ratio themselves.
   */
  score: number;
  /** Pattern name (if provided). */
  name?: string;
};

// -- Internal helpers --------------------------------

const resolveDistance = (
  dist: number | "auto",
  patternLength: number,
): number => {
  if (dist !== "auto") return dist;
  if (patternLength <= 2) return 0;
  if (patternLength <= 5) return 1;
  return 2;
};

const normalizeEntry = (
  p: PatternEntry,
  i: number,
): NormalizedEntry => {
  if (typeof p === "string") {
    return { pattern: p };
  }
  if (
    typeof p === "object" &&
    p !== null &&
    typeof p.pattern === "string"
  ) {
    if (p.distance === "auto") {
      return {
        ...p,
        distance: resolveDistance("auto", p.pattern.length),
      };
    }
    // SAFETY: The "auto" case was already handled above,
    // so p.distance is number | undefined — matching
    // NormalizedEntry.
    return p as NormalizedEntry;
  }
  throw new TypeError(
    `Pattern at index ${i} must be a string ` +
      `or { pattern, distance?, name? }`,
  );
};

/** Score formula: clamped `1 - distance / patternLength`. */
const computeScore = (
  distance: number,
  patternLength: number,
): number => {
  if (patternLength <= 0) return 0;
  const raw = 1 - distance / patternLength;
  return raw < 0 ? 0 : raw;
};

/**
 * Stable ranking for `kBest`: higher score wins;
 * ties go to lower `start`, then pattern
 * index ascending.
 */
const compareForKBest = (
  a: FuzzyMatch,
  b: FuzzyMatch,
): number => {
  if (a.score !== b.score) return b.score - a.score;
  if (a.start !== b.start) return a.start - b.start;
  return a.pattern - b.pattern;
};

const unpack = (
  packed: Uint32Array,
  haystack: string,
  patterns: string[],
  names: (string | undefined)[],
): FuzzyMatch[] => {
  const len = packed.length;
  const matches: FuzzyMatch[] = [];
  for (let i = 0; i < len; i += 4) {
    const idx = packed[i];
    const start = packed[i + 1];
    const end = packed[i + 2];
    const distance = packed[i + 3];
    if (
      idx === undefined ||
      start === undefined ||
      end === undefined ||
      distance === undefined
    ) {
      throw new Error(
        `Malformed packed array at offset ${String(i)}`,
      );
    }
    const pat = patterns[idx];
    if (pat === undefined) {
      throw new Error(
        `Malformed packed array: pattern index ${String(idx)} out of range`,
      );
    }
    const m: FuzzyMatch = {
      pattern: idx,
      start,
      end,
      text: haystack.slice(start, end),
      distance,
      score: computeScore(distance, pat.length),
    };
    if (names[idx] !== undefined) {
      m.name = names[idx];
    }
    matches.push(m);
  }
  return matches;
};

// -- Classes -----------------------------------------

/**
 * Fuzzy string matcher. Finds approximate
 * matches within edit distance k, immune to
 * typos, OCR errors, and diacritics variants.
 *
 * Uses Myers' bit-parallel algorithm for O(n)
 * scanning per pattern (patterns up to 64 chars).
 *
 * @throws {Error} If a pattern is empty, too
 *   long (> 64 chars), or distance > 3.
 *
 * @example
 * ```ts
 * const fs = new FuzzySearch([
 *   { pattern: "Gaislerova", distance: 1 },
 *   { pattern: "Novak", distance: 1 },
 * ], {
 *   normalizeDiacritics: true,
 *   wholeWords: true,
 * });
 *
 * fs.findIter("Gais1erova a Nowak");
 * // [
 * //   { pattern: 0, start: 0, end: 10,
 * //     text: "Gais1erova", distance: 1 },
 * //   { pattern: 1, start: 13, end: 18,
 * //     text: "Nowak", distance: 1 },
 * // ]
 * ```
 */
export class FuzzySearch {
  private _patterns: string[];
  private _names: (string | undefined)[];
  private _minScore: number | undefined;
  private _kBest: number | undefined;
  private _inner: NativeFuzzySearch;

  constructor(patterns: PatternEntry[], options?: Options) {
    const entries = patterns.map(normalizeEntry);
    this._patterns = entries.map((e) => e.pattern);
    this._names = entries.map((e) => e.name);
    this._minScore = options?.minScore;
    this._kBest = options?.kBest;
    this._inner = new binding.FuzzySearch(entries, options);
  }

  /** Number of patterns in the matcher. */
  get patternCount(): number {
    return this._inner.patternCount;
  }

  /**
   * Returns `true` if any pattern matches
   * within its edit distance. Not affected by
   * `minScore` or `kBest`.
   */
  isMatch(haystack: string): boolean {
    return this._inner.isMatch(haystack);
  }

  /**
   * Find non-overlapping fuzzy matches.
   *
   * Without `minScore` or `kBest`, matches are
   * returned in ascending `start` order. With
   * `kBest`, matches are returned in
   * score-descending order (ties broken by
   * `start`, then pattern index).
   */
  findIter(haystack: string): FuzzyMatch[] {
    const matches = unpack(
      this._inner._findIterPacked(haystack),
      haystack,
      this._patterns,
      this._names,
    );
    const minScore = this._minScore;
    const filtered =
      minScore === undefined
        ? matches
        : matches.filter((m) => m.score >= minScore);
    const kBest = this._kBest;
    if (kBest === undefined) return filtered;
    if (kBest <= 0) return [];
    const sorted = filtered.sort(compareForKBest);
    return sorted.length <= kBest
      ? sorted
      : sorted.slice(0, kBest);
  }

  /**
   * Replace all fuzzy matches.
   * `replacements[i]` replaces pattern `i`.
   *
   * Always replaces every distance-qualified
   * match; ignores `minScore` and `kBest` so the
   * `replacements`-by-pattern contract stays
   * deterministic.
   *
   * @throws {Error} If `replacements.length`
   *   does not equal `patternCount`.
   */
  replaceAll(
    haystack: string,
    replacements: string[],
  ): string {
    return this._inner.replaceAll(haystack, replacements);
  }
}

/**
 * Compute edit distance between two strings.
 *
 * Uses Unicode characters (not UTF-16 code units),
 * so emoji and supplementary plane characters are
 * handled correctly.
 *
 * @example
 * ```ts
 * distance("Novak", "Nowak");       // 1
 * distance("abcd", "abdc");         // 2
 * distance("abcd", "abdc",
 *   "damerau-levenshtein");          // 1
 * ```
 */
export const distance = (
  a: string,
  b: string,
  metric?: Metric,
): number => binding.distance(a, b, metric ?? null);
