/** Distance metric for fuzzy matching. */
export type Metric =
  | "levenshtein"
  | "damerau-levenshtein";

/** Options for constructing a `FuzzySearch`. */
export type Options = {
  /**
   * Distance metric.
   * - `"levenshtein"`: insertions, deletions,
   *   substitutions (default).
   * - `"damerau-levenshtein"`: + transpositions
   *   of adjacent characters (ab → ba = 1 edit).
   * @default "levenshtein"
   */
  metric?: Metric;
  /**
   * Strip diacritics before matching (NFD
   * decompose + remove combining marks).
   * "Příbram" matches "Pribram" at distance 0.
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
       *  Elasticsearch convention: 1-2 chars → 0,
       *  3-5 chars → 1, 6+ chars → 2.
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
 *   { pattern: "Gaislerová", distance: 1 },
 *   { pattern: "Novák", distance: 1 },
 * ], {
 *   normalizeDiacritics: true,
 *   wholeWords: true,
 * });
 *
 * fs.findIter("Gais1erová a Nowák");
 * // [
 * //   { pattern: 0, start: 0, end: 10,
 * //     text: "Gais1erová", distance: 1 },
 * //   { pattern: 1, start: 13, end: 18,
 * //     text: "Nowák", distance: 1 },
 * // ]
 * ```
 */
/**
 * Compute edit distance between two strings.
 *
 * Uses Unicode characters (not UTF-16 code units),
 * so emoji and supplementary plane characters are
 * handled correctly — unlike `js-levenshtein` which
 * counts surrogate pairs as two characters.
 *
 * @example
 * ```ts
 * distance("Novák", "Nowák");       // 1
 * distance("abcd", "abdc");         // 2
 * distance("abcd", "abdc",
 *   "damerau-levenshtein");          // 1
 * distance("😀x", "😀y");           // 1
 * ```
 */
export declare function distance(
  a: string,
  b: string,
  metric?: Metric,
): number;

export declare class FuzzySearch {
  constructor(
    patterns: PatternEntry[],
    options?: Options,
  );

  /** Number of patterns in the matcher. */
  get patternCount(): number;

  /**
   * Returns `true` if any pattern matches
   * within its edit distance.
   */
  isMatch(haystack: string): boolean;

  /** Find all non-overlapping fuzzy matches. */
  findIter(haystack: string): FuzzyMatch[];

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
  ): string;
}
