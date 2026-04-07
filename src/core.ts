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

const unpack = (
  packed: Uint32Array,
  haystack: string,
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
    const m: FuzzyMatch = {
      pattern: idx,
      start,
      end,
      text: haystack.slice(start, end),
      distance,
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
 *   long (> 64 chars), or has distance >=
 *   pattern length.
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
  private _names: (string | undefined)[];
  private _inner: NativeFuzzySearch;

  constructor(patterns: PatternEntry[], options?: Options) {
    const entries = patterns.map(normalizeEntry);
    this._names = entries.map((e) => e.name);
    this._inner = new binding.FuzzySearch(entries, options);
  }

  /** Number of patterns in the matcher. */
  get patternCount(): number {
    return this._inner.patternCount;
  }

  /**
   * Returns `true` if any pattern matches
   * within its edit distance.
   */
  isMatch(haystack: string): boolean {
    return this._inner.isMatch(haystack);
  }

  /** Find all non-overlapping fuzzy matches. */
  findIter(haystack: string): FuzzyMatch[] {
    return unpack(
      this._inner._findIterPacked(haystack),
      haystack,
      this._names,
    );
  }

  /**
   * Replace all fuzzy matches.
   * `replacements[i]` replaces pattern `i`.
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
