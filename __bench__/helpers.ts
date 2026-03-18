import Fuse from "fuse.js";
import { distance as fastLev } from "fastest-levenshtein";
// eslint-disable-next-line @typescript-eslint/no-require-imports
const fuzzball = require("fuzzball");

import { FuzzySearch } from "../lib";

// ─── Adapter type ─────────────────────────────

export type PatternDef = {
  pattern: string;
  distance: number;
};

export type Lib = {
  name: string;
  build: (p: PatternDef[]) => unknown;
  search: (engine: unknown, h: string) => number;
};

// ─── Naive Levenshtein (reference) ────────────

function naiveLevenshtein(
  a: string,
  b: string,
): number {
  const m = a.length;
  const n = b.length;
  if (m === 0) return n;
  if (n === 0) return m;
  let prev = Array.from(
    { length: n + 1 },
    (_, i) => i,
  );
  for (let i = 1; i <= m; i++) {
    const curr = new Array<number>(n + 1);
    curr[0] = i;
    for (let j = 1; j <= n; j++) {
      const cost =
        a[i - 1] === b[j - 1] ? 0 : 1;
      curr[j] = Math.min(
        curr[j - 1]! + 1,
        prev[j]! + 1,
        prev[j - 1]! + cost,
      );
    }
    prev = curr;
  }
  return prev[n]!;
}

/** Naive sliding window fuzzy search (correct
 *  but slow). O(n × k × m²) per pattern. */
export function naiveSlidingWindow(
  patterns: PatternDef[],
  text: string,
): number {
  let count = 0;
  for (const { pattern, distance } of patterns) {
    const m = pattern.length;
    const minLen = Math.max(1, m - distance);
    const maxLen = m + distance;
    let lastEnd = 0;
    for (let i = 0; i <= text.length - minLen; i++) {
      if (i < lastEnd) continue;
      for (let len = minLen; len <= maxLen && i + len <= text.length; len++) {
        const window = text.slice(i, i + len);
        if (
          naiveLevenshtein(pattern, window) <=
          distance
        ) {
          count++;
          lastEnd = i + len;
          break;
        }
      }
    }
  }
  return count;
}

/** Sliding window using fastest-levenshtein. */
export function fastLevSlidingWindow(
  patterns: PatternDef[],
  text: string,
): number {
  let count = 0;
  for (const { pattern, distance } of patterns) {
    const m = pattern.length;
    const minLen = Math.max(1, m - distance);
    const maxLen = m + distance;
    let lastEnd = 0;
    for (let i = 0; i <= text.length - minLen; i++) {
      if (i < lastEnd) continue;
      for (let len = minLen; len <= maxLen && i + len <= text.length; len++) {
        const window = text.slice(i, i + len);
        if (fastLev(pattern, window) <= distance) {
          count++;
          lastEnd = i + len;
          break;
        }
      }
    }
  }
  return count;
}

// ─── All adapters ─────────────────────────────

export const libs: Lib[] = [
  {
    name: "@stll/fuzzy-search",
    build: (p) =>
      new FuzzySearch(
        p.map((x) => ({
          pattern: x.pattern,
          distance: x.distance,
        })),
        { wholeWords: false },
      ),
    search: (engine, h) =>
      (engine as FuzzySearch).findIter(h).length,
  },
  {
    name: "fastest-levenshtein + window",
    build: (p) => p,
    search: (p, h) =>
      fastLevSlidingWindow(
        p as PatternDef[],
        h,
      ),
  },
  {
    name: "naive JS (sliding window)",
    build: (p) => p,
    search: (p, h) =>
      naiveSlidingWindow(p as PatternDef[], h),
  },
  {
    name: "fuse.js (word-split)",
    build: (p) => ({
      patterns: p,
      // Pre-split not possible since haystack
      // varies. Build Fuse per search.
    }),
    search: (ctx, h) => {
      const { patterns } = ctx as {
        patterns: PatternDef[];
      };
      // Split text into words, search each
      // pattern in the word list.
      const words = h.split(/\s+/).map(
        (w: string, i: number) => ({
          word: w,
          idx: i,
        }),
      );
      const fuse = new Fuse(words, {
        keys: ["word"],
        threshold: 0.4,
        includeScore: true,
        distance: 100,
      });
      let count = 0;
      for (const p of patterns) {
        count += fuse.search(p.pattern).length;
      }
      return count;
    },
  },
  {
    name: "fuzzball.extract",
    build: (p) => p,
    search: (p, h) => {
      const patterns = p as PatternDef[];
      const words = h.split(/\s+/);
      let count = 0;
      for (const pat of patterns) {
        const results = fuzzball.extract(
          pat.pattern,
          words,
          { scorer: fuzzball.ratio, cutoff: 70 },
        );
        count += results.length;
      }
      return count;
    },
  },
];

// ─── Bench runner ─────────────────────────────

export const bench = (
  name: string,
  fn: () => number,
  n: number,
) => {
  for (let i = 0; i < 2; i++) fn();
  const t = performance.now();
  let c = 0;
  for (let i = 0; i < n; i++) c = fn();
  const ms = (performance.now() - t) / n;
  console.log(
    `  ${name.padEnd(36)}` +
      `${ms.toFixed(2).padStart(10)} ms ` +
      `${String(c).padStart(8)} matches`,
  );
  return ms;
};

export const printSpeedups = (
  times: number[],
) => {
  const stellaMs = times[0]!;
  console.log();
  for (let i = 1; i < times.length; i++) {
    const lib = libs[i];
    if (lib && times[i] !== undefined) {
      console.log(
        `  vs ${lib.name}: ` +
          `${(times[i]! / stellaMs).toFixed(1)}x`,
      );
    }
  }
};

// ─── Shared pattern sets ──────────────────────

export const CZECH_NAMES: PatternDef[] = [
  { pattern: "Gaislerová", distance: 1 },
  { pattern: "Novák", distance: 1 },
  { pattern: "Šnytrová", distance: 1 },
  { pattern: "Příbram", distance: 2 },
  { pattern: "Dvořák", distance: 1 },
];

export const ENGLISH_NAMES: PatternDef[] = [
  { pattern: "Johnson", distance: 1 },
  { pattern: "Williams", distance: 1 },
  { pattern: "Thompson", distance: 1 },
  { pattern: "Anderson", distance: 1 },
  { pattern: "Robertson", distance: 2 },
];

export { FuzzySearch };
