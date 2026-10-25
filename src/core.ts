/* Shared core: types, helpers, and classes that
 * use a late-bound native backend (NAPI-RS or WASM).
 * Call initBinding() before constructing classes. */

// -- Native binding types ----------------------------

export type NativeBinding = {
  EntityIndexBuilder: new (
    options: EntityIndexOptions,
  ) => NativeEntityIndexBuilder;
  EntityIndex: {
    fromBytes(bytes: Uint8Array): NativeEntityIndex;
  };
  FuzzySearch: new (
    entries: NormalizedEntry[],
    options?: Options,
  ) => NativeFuzzySearch;
  distance: (a: string, b: string, metric: Metric | null) => number;
};

type NativeFuzzySearch = {
  patternCount: number;
  isMatch(haystack: string): boolean;
  _findIterPacked(haystack: string): Uint32Array;
  replaceAll(haystack: string, replacements: string[]): string;
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
export const initBinding = (b: NativeBinding): (() => void) => {
  const previous = binding;
  binding = b;
  return () => {
    binding = previous;
  };
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

const normalizeEntry = (p: unknown, i: number): NormalizedEntry => {
  if (typeof p === "string") {
    return { pattern: p };
  }

  if (typeof p !== "object" || p === null || !("pattern" in p)) {
    throw new TypeError(
      `Pattern at index ${i} must be a string ` +
        `or { pattern, distance?, name? }`,
    );
  }

  if (typeof p.pattern !== "string") {
    throw new TypeError(
      `Pattern at index ${i}: "pattern" field must be a string`,
    );
  }

  const entry: NormalizedEntry = { pattern: p.pattern };

  if ("distance" in p && p.distance !== undefined) {
    if (p.distance === "auto") {
      entry.distance = resolveDistance("auto", p.pattern.length);
    } else if (typeof p.distance === "number") {
      if (!Number.isInteger(p.distance) || p.distance < 0) {
        throw new TypeError(
          `Pattern at index ${i}: "distance" field must be a non-negative integer`,
        );
      }
      entry.distance = p.distance;
    } else {
      throw new TypeError(
        `Pattern at index ${i}: "distance" field must be a number or "auto"`,
      );
    }
  }

  if ("name" in p && p.name !== undefined) {
    if (typeof p.name !== "string") {
      throw new TypeError(
        `Pattern at index ${i}: "name" field must be a string`,
      );
    }
    entry.name = p.name;
  }

  return entry;
};

/** Score formula: clamped `1 - distance / patternLength`. */
const computeScore = (distance: number, patternLength: number): number => {
  if (patternLength <= 0) return 0;
  const raw = 1 - distance / patternLength;
  return raw < 0 ? 0 : raw;
};

/**
 * Stable ranking for `kBest`: higher score wins;
 * ties go to lower `start`, then pattern
 * index ascending.
 */
const compareForKBest = (a: FuzzyMatch, b: FuzzyMatch): number => {
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
      throw new Error(`Malformed packed array at offset ${String(i)}`);
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
    return sorted.length <= kBest ? sorted : sorted.slice(0, kBest);
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
  replaceAll(haystack: string, replacements: string[]): string {
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
export const distance = (a: string, b: string, metric?: Metric): number =>
  binding.distance(a, b, metric ?? null);

/** Construction options. All text must already be normalized by the caller. */
export type EntityIndexOptions = {
  forms: number;
  budget: { byLength: number[]; basis: "shorter" };
  initialWeight: number;
  initialSimilarity: number;
};
export type EntityAliasToken = {
  id: number;
  weight: number;
  minimumWeight: number;
};
export type EntityAliasJoin = {
  id: number;
  left: number;
  right: number;
};
export type EntityAliasForm = {
  form: number;
  tokens: EntityAliasToken[];
  joins: EntityAliasJoin[];
  minimumTotal: number;
};
export type EntityAliasInput = {
  entryId: number;
  entityGroup: number;
  quality: number;
  forms: EntityAliasForm[];
};
export type EntityQuery = {
  tokens: { form: number; text: string; weight: number }[];
  units: {
    form: number;
    text: string;
    positions: number[];
  }[];
};
export type EntitySearchParams = {
  /** Candidate bound threshold; final scoring remains caller owned. */
  tau: number;
  quality: number;
  /** Non-negative safe integer. Exhaustion means results may be incomplete. */
  maxCost: number;
};
export type EntitySearchCost = {
  lexiconCells: number;
  lexiconMatches: number;
  sortedAccesses: number;
  randomAccessPairs: number;
  survivors: number;
};
export type EntitySearchResult = {
  /** [unit, form, token id] triples, aligned with similarities. */
  matchKeys: Uint32Array;
  similarities: Float64Array;
  queryWeights: Float64Array;
  /** Surviving alias ids; ids follow builder insertion order. */
  survivors: Uint32Array;
  cost: EntitySearchCost;
  exhausted: boolean;
};
type NativeEntitySearchResult = Omit<EntitySearchResult, "cost"> & {
  cost: Float64Array;
};
type NativeEntityIndex = {
  nativeBytes: number;
  postingCount: number;
  toBytes(): Uint8Array;
  _searchPacked(
    query: EntityQuery,
    params: EntitySearchParams,
  ): NativeEntitySearchResult;
};
type NativeEntityIndexBuilder = {
  intern(form: number, text: string): number;
  addAlias(alias: EntityAliasInput): number;
  finishStep(maxUnits: number): boolean;
  finish(weights: Float64Array[]): NativeEntityIndex;
  finishAsync(weights: Float64Array[]): Promise<NativeEntityIndex>;
};

const MAX_U32 = 0xffff_ffff;
const MAX_U8 = 0xff;

type UnsignedIntegerOptions = {
  value: number;
  maximum: number;
  field: string;
};
const assertUnsignedInteger = ({
  value,
  maximum,
  field,
}: UnsignedIntegerOptions) => {
  if (!Number.isInteger(value) || value < 0 || value > maximum) {
    throw new RangeError(
      `${field} must be an integer in [0, ${String(maximum)}]`,
    );
  }
};

const validateEntityAlias = (alias: EntityAliasInput) => {
  assertUnsignedInteger({
    value: alias.entryId,
    maximum: MAX_U32,
    field: "entryId",
  });
  assertUnsignedInteger({
    value: alias.entityGroup,
    maximum: MAX_U32,
    field: "entityGroup",
  });
  for (const form of alias.forms) {
    assertUnsignedInteger({
      value: form.form,
      maximum: MAX_U32,
      field: "form",
    });
    for (const token of form.tokens)
      assertUnsignedInteger({
        value: token.id,
        maximum: MAX_U32,
        field: "token id",
      });
    for (const join of form.joins) {
      assertUnsignedInteger({
        value: join.id,
        maximum: MAX_U32,
        field: "join id",
      });
      assertUnsignedInteger({
        value: join.left,
        maximum: MAX_U32,
        field: "join left",
      });
      assertUnsignedInteger({
        value: join.right,
        maximum: MAX_U32,
        field: "join right",
      });
    }
  }
};

const validateEntityQuery = (query: EntityQuery) => {
  for (const token of query.tokens)
    assertUnsignedInteger({
      value: token.form,
      maximum: MAX_U32,
      field: "form",
    });
  for (const unit of query.units) {
    assertUnsignedInteger({
      value: unit.form,
      maximum: MAX_U32,
      field: "form",
    });
    for (const position of unit.positions)
      assertUnsignedInteger({
        value: position,
        maximum: MAX_U32,
        field: "position",
      });
  }
};

/** Mutable index construction; finishing consumes the builder, even on failure. */
export class EntityIndexBuilder {
  private _inner: NativeEntityIndexBuilder;

  constructor(options: EntityIndexOptions) {
    assertUnsignedInteger({
      value: options.forms,
      maximum: MAX_U32,
      field: "forms",
    });
    for (const budget of options.budget.byLength)
      assertUnsignedInteger({
        value: budget,
        maximum: MAX_U8,
        field: "budget",
      });
    this._inner = new binding.EntityIndexBuilder(options);
  }
  intern(form: number, text: string): number {
    assertUnsignedInteger({
      value: form,
      maximum: MAX_U32,
      field: "form",
    });
    return this._inner.intern(form, text);
  }
  addAlias(alias: EntityAliasInput): number {
    validateEntityAlias(alias);
    return this._inner.addAlias(alias);
  }
  finish(tokenWeights: Float64Array[]): EntityIndex {
    return EntityIndex._fromNative(this._inner.finish(tokenWeights));
  }
  /** Build on the native worker pool; weights are copied before returning. */
  async finishAsync(tokenWeights: Float64Array[]): Promise<EntityIndex> {
    return EntityIndex._fromNative(await this._inner.finishAsync(tokenWeights));
  }
  /** Advance construction by at most maxUnits work units. */
  finishStep(maxUnits: number): boolean {
    assertUnsignedInteger({
      value: maxUnits,
      maximum: MAX_U32,
      field: "maxUnits",
    });
    if (maxUnits === 0) throw new RangeError("maxUnits must be positive");
    return this._inner.finishStep(maxUnits);
  }
  /** Yield between bounded construction steps, then return the finished index. */
  *finishSteps(
    tokenWeights: Float64Array[],
    maxUnits = 10_000,
  ): Generator<void, EntityIndex> {
    if (!Number.isInteger(maxUnits) || maxUnits <= 0 || maxUnits > MAX_U32) {
      throw new RangeError("maxUnits must be a positive u32 integer");
    }
    while (!this.finishStep(maxUnits)) yield;
    return this.finish(tokenWeights);
  }
}

/** Immutable candidate index. Search returns packed hits without per-hit objects. */
export class EntityIndex {
  private _inner: NativeEntityIndex;

  private constructor(inner: NativeEntityIndex) {
    this._inner = inner;
  }
  /** @internal */
  static _fromNative(inner: NativeEntityIndex): EntityIndex {
    return new EntityIndex(inner);
  }
  static fromBytes(bytes: Uint8Array): EntityIndex {
    return new EntityIndex(binding.EntityIndex.fromBytes(bytes));
  }
  toBytes(): Uint8Array {
    return this._inner.toBytes();
  }
  get nativeBytes(): number {
    return this._inner.nativeBytes;
  }
  get postingCount(): number {
    return this._inner.postingCount;
  }
  search(query: EntityQuery, params: EntitySearchParams): EntitySearchResult {
    validateEntityQuery(query);
    const result = this._inner._searchPacked(query, params);
    const [
      lexiconCells,
      lexiconMatches,
      sortedAccesses,
      randomAccessPairs,
      survivors,
    ] = result.cost;
    if (
      result.cost.length !== 5 ||
      lexiconCells === undefined ||
      lexiconMatches === undefined ||
      sortedAccesses === undefined ||
      randomAccessPairs === undefined ||
      survivors === undefined ||
      result.matchKeys.length !== result.similarities.length * 3
    ) {
      throw new Error("Malformed packed entity result");
    }
    return {
      matchKeys: result.matchKeys,
      similarities: result.similarities,
      queryWeights: result.queryWeights,
      survivors: result.survivors,
      cost: {
        lexiconCells,
        lexiconMatches,
        sortedAccesses,
        randomAccessPairs,
        survivors,
      },
      exhausted: result.exhausted,
    };
  }
}
