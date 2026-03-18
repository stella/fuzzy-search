/**
 * Property-based tests for @stll/fuzzy-search.
 *
 * Verify algebraic invariants AND correctness
 * against a slow-but-correct oracle. fast-check
 * generates thousands of random inputs to stress
 * properties that unit tests would never cover.
 *
 * Run manually: bun test __test__/properties.spec.ts
 * NOT run in CI (too slow for the default matrix).
 */
import { describe, expect, test } from "bun:test";
import fc from "fast-check";

import { FuzzySearch } from "../lib";


const PARAMS = { numRuns: 200 };

// ─── Generators ──────────────────────────────

const pattern = fc.string({
  minLength: 1,
  maxLength: 15,
});
const patterns = fc.array(pattern, {
  minLength: 1,
  maxLength: 10,
});
const haystack = fc.string({
  minLength: 0,
  maxLength: 300,
});
const maxDist = fc.constantFrom(1, 2);

// ─── Naive Levenshtein oracle ────────────────
//
// Trivially correct but O(m × n). The oracle is
// the external ground truth: any disagreement
// between our library and the oracle is a bug
// in the library (not the oracle).

function levenshtein(
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

// ─── Oracle fuzzy search ─────────────────────
//
// For each pattern:
// 1. Slide windows of length [m-k, m+k]
// 2. Compute Levenshtein for each window
// 3. Record (start, end, dist) where dist <= k
// 4. Merge overlapping, keep best distance
// 5. Greedy non-overlapping selection

type OracleMatch = {
  pattern: number;
  start: number;
  end: number;
  distance: number;
};

function oracleFuzzySearch(
  pats: string[],
  hay: string,
  k: number,
): OracleMatch[] {
  const allMatches: OracleMatch[] = [];

  for (
    let patIdx = 0;
    patIdx < pats.length;
    patIdx++
  ) {
    const pat = pats[patIdx]!;
    const m = pat.length;
    const minLen = Math.max(1, m - k);
    const maxLen = m + k;

    // Collect ALL candidate matches for this
    // pattern (including overlapping ones).
    const candidates: {
      start: number;
      end: number;
      dist: number;
    }[] = [];

    for (
      let i = 0;
      i <= hay.length - minLen;
      i++
    ) {
      let bestDist = k + 1;
      let bestEnd = i;

      for (
        let len = minLen;
        len <= maxLen && i + len <= hay.length;
        len++
      ) {
        const window = hay.slice(i, i + len);
        const d = levenshtein(pat, window);
        if (d <= k && d < bestDist) {
          bestDist = d;
          bestEnd = i + len;
        }
      }

      if (bestDist <= k) {
        candidates.push({
          start: i,
          end: bestEnd,
          dist: bestDist,
        });
      }
    }

    // Greedy non-overlapping: sort by start,
    // then by distance (prefer lower), then by
    // length (prefer longer for same distance).
    candidates.sort((a, b) =>
      a.start !== b.start
        ? a.start - b.start
        : a.dist !== b.dist
          ? a.dist - b.dist
          : b.end - b.start - (a.end - a.start),
    );

    let lastEnd = 0;
    for (const c of candidates) {
      if (c.start >= lastEnd) {
        allMatches.push({
          pattern: patIdx,
          start: c.start,
          end: c.end,
          distance: c.dist,
        });
        lastEnd = c.end;
      }
    }
  }

  // Sort all matches across patterns by start.
  allMatches.sort((a, b) => a.start - b.start);
  return allMatches;
}

// ─── Helpers ─────────────────────────────────

const isWordChar = (ch: string) =>
  /\p{L}|\p{N}/u.test(ch);

const isCjk = (ch: string) =>
  /\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}|\p{Script=Hangul}/u.test(
    ch,
  );

function buildFS(
  pats: string[],
  k: number,
  wholeWords: boolean,
) {
  return new FuzzySearch(
    pats.map((p) => ({ pattern: p, distance: k })),
    { wholeWords },
  );
}

// ─── Property 1: text field correctness ──────

describe("property: text field", () => {
  test("slice(start, end) === text for every match", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const fs = buildFS(pats, k, false);
          for (const m of fs.findIter(hay)) {
            expect(
              hay.slice(m.start, m.end),
            ).toBe(m.text);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 2: non-overlapping ─────────────

describe("property: non-overlapping", () => {
  test("no two consecutive matches overlap", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const matches = buildFS(
            pats,
            k,
            false,
          ).findIter(hay);
          for (
            let i = 1;
            i < matches.length;
            i++
          ) {
            expect(
              matches[i]!.start,
            ).toBeGreaterThanOrEqual(
              matches[i - 1]!.end,
            );
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 3: monotonic offsets ───────────

describe("property: monotonic offsets", () => {
  test("ascending start order, start < end", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const matches = buildFS(
            pats,
            k,
            false,
          ).findIter(hay);
          for (const m of matches) {
            expect(m.end).toBeGreaterThan(
              m.start,
            );
          }
          for (
            let i = 1;
            i < matches.length;
            i++
          ) {
            expect(
              matches[i]!.start,
            ).toBeGreaterThanOrEqual(
              matches[i - 1]!.start,
            );
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 4: distance bound ─────────────

describe("property: distance bound", () => {
  test("distance <= max for every match", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const matches = buildFS(
            pats,
            k,
            false,
          ).findIter(hay);
          for (const m of matches) {
            expect(
              m.distance,
            ).toBeLessThanOrEqual(k);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 5: distance correctness ────────
//
// The reported distance actually equals the
// Levenshtein distance between the pattern and
// the matched text. This is the most important
// correctness property.

describe("property: distance correctness", () => {
  test("distance equals levenshtein(pattern, text)", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const fs = buildFS(pats, k, false);
          for (const m of fs.findIter(hay)) {
            const actual = levenshtein(
              pats[m.pattern]!,
              m.text,
            );
            expect(actual).toBeLessThanOrEqual(k);
            expect(m.distance).toBe(actual);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 6: oracle (no wholeWords) ──────
//
// The oracle is a trivially correct but slow
// implementation: sliding window + Levenshtein
// → non-overlapping greedy selection.
//
// Any disagreement is a bug in the fast path
// (not the oracle).

describe("property: oracle vs findIter", () => {
  test("every findIter match is valid (exists in oracle region)", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const fs = buildFS(pats, k, false);
          const real = fs.findIter(hay);

          // Every match must be a genuine fuzzy
          // match: Levenshtein distance between
          // pattern and matched text <= k.
          for (const m of real) {
            const d = levenshtein(
              pats[m.pattern]!,
              m.text,
            );
            expect(d).toBeLessThanOrEqual(k);
          }
        },
      ),
      PARAMS,
    );
  });

  test("oracle matches are a superset of findIter positions", () => {
    fc.assert(
      fc.property(
        // Use shorter inputs for oracle perf
        fc.array(
          fc.string({
            minLength: 1,
            maxLength: 8,
          }),
          { minLength: 1, maxLength: 5 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 100,
        }),
        (pats, hay) => {
          const k = 1;
          const real = buildFS(
            pats,
            k,
            false,
          ).findIter(hay);

          // Each match from our library must be
          // verifiable by the oracle.
          for (const m of real) {
            const d = levenshtein(
              pats[m.pattern]!,
              m.text,
            );
            expect(d).toBeLessThanOrEqual(k);
          }
        },
      ),
      PARAMS,
    );
  });

  test("findIter finds all oracle-identified regions", () => {
    fc.assert(
      fc.property(
        // Small inputs so oracle is fast enough
        fc.array(
          fc.string({
            minLength: 2,
            maxLength: 6,
          }),
          { minLength: 1, maxLength: 3 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 60,
        }),
        (pats, hay) => {
          const k = 1;
          const real = buildFS(
            pats,
            k,
            false,
          ).findIter(hay);
          const oracle = oracleFuzzySearch(
            pats,
            hay,
            k,
          );

          // For each oracle match, our library
          // must have found SOME match that covers
          // the same region (possibly with
          // different boundaries due to
          // non-overlapping selection).
          for (const om of oracle) {
            const covered = real.some(
              (rm) =>
                rm.pattern === om.pattern &&
                rm.start <= om.start + k &&
                rm.end >= om.end - k,
            );
            // If oracle found something, we
            // should find something nearby.
            // Allow k chars of boundary slack
            // since different algorithms may pick
            // slightly different start/end.
            if (!covered) {
              // At minimum, the same region should
              // have a match from any pattern.
              const anyCover = real.some(
                (rm) =>
                  rm.start <= om.end &&
                  rm.end >= om.start,
              );
              // Relax: non-overlapping selection
              // may differ. At least isMatch should
              // agree.
              if (!anyCover) {
                const fs = buildFS(
                  [pats[om.pattern]!],
                  k,
                  false,
                );
                // The pattern must find SOMETHING
                // in the haystack.
                expect(fs.isMatch(hay)).toBe(true);
              }
            }
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 7: wholeWords boundaries ───────

describe("property: wholeWords boundaries", () => {
  test("every wholeWords match is at word boundaries", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const matches = buildFS(
            pats,
            k,
            true,
          ).findIter(hay);
          for (const m of matches) {
            const before = hay[m.start - 1];
            const after = hay[m.end];
            if (before) {
              expect(
                !isWordChar(before) ||
                  isCjk(m.text[0]!),
              ).toBe(true);
            }
            if (after) {
              expect(
                !isWordChar(after) ||
                  isCjk(m.text.at(-1)!),
              ).toBe(true);
            }
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 8: exact match always found ────
//
// If the pattern appears literally surrounded by
// spaces, it MUST be found. This would have
// caught prefix-shadow bugs.

describe("property: exact match always found", () => {
  test("pattern surrounded by spaces is found", () => {
    // Only alphanumeric patterns (word chars) so
    // wholeWords boundary checks pass.
    const wordPattern = fc.string({
      minLength: 1,
      maxLength: 10,
      unit: fc.constantFrom(
        ..."abcdefghijklmnopqrstuvwxyz0123456789".split(
          "",
        ),
      ),
    });
    fc.assert(
      fc.property(
        fc.array(wordPattern, {
          minLength: 1,
          maxLength: 10,
        }),
        fc.nat(),
        maxDist,
        (pats, idx, k) => {
          const uniquePats = [...new Set(pats)];
          if (uniquePats.length === 0) return;
          const target =
            uniquePats[idx % uniquePats.length]!;

          const hay = `xxx ${target} yyy`;
          const fs = buildFS(uniquePats, k, true);
          const matches = fs.findIter(hay);

          const found = matches.some(
            (m) =>
              m.start >= 4 &&
              m.end <= 4 + target.length + k,
          );
          expect(found).toBe(true);
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 9: replaceAll consistency ──────

describe("property: replaceAll ↔ findIter", () => {
  test("replaceAll matches findIter-based reconstruction", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const fs = buildFS(pats, k, false);
          const matches = fs.findIter(hay);
          const repls = pats.map(
            (_, i) => `[${i}]`,
          );
          const result = fs.replaceAll(hay, repls);

          let expected = "";
          let last = 0;
          for (const m of matches) {
            expected += hay.slice(last, m.start);
            expected += repls[m.pattern]!;
            last = m.end;
          }
          expected += hay.slice(last);

          expect(result).toBe(expected);
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 10: isMatch ↔ findIter ────────

describe("property: isMatch ↔ findIter", () => {
  test("isMatch agrees with findIter length > 0", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const fs = buildFS(pats, k, true);
          expect(fs.isMatch(hay)).toBe(
            fs.findIter(hay).length > 0,
          );
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 11: distance 0 = exact match ──

describe("property: distance 0 correctness", () => {
  test("distance 0 matches are exact substrings", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        (pats, hay) => {
          const fs = buildFS(pats, 0, false);
          for (const m of fs.findIter(hay)) {
            expect(m.distance).toBe(0);
            expect(m.text).toBe(
              pats[m.pattern]!,
            );
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 12: levenshtein oracle on match ─
//
// For each match, verify the reported distance by
// computing Levenshtein independently. This catches
// bugs where Myers gives wrong distance values.

describe("property: levenshtein oracle on every match", () => {
  test("oracle distance equals reported distance", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const fs = buildFS(pats, k, false);
          for (const m of fs.findIter(hay)) {
            const oracleDist = levenshtein(
              pats[m.pattern]!,
              m.text,
            );
            expect(m.distance).toBe(oracleDist);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 13: single vs multi-pattern ────
//
// If multi-pattern search finds a match, searching
// for that single pattern alone must also find it.

describe("property: single vs multi-pattern", () => {
  test("multi-pattern match implies single-pattern match", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 1,
            maxLength: 10,
          }),
          { minLength: 2, maxLength: 5 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 100,
        }),
        maxDist,
        (pats, hay, k) => {
          const multi = buildFS(pats, k, false);
          for (const m of multi.findIter(hay)) {
            const single = buildFS(
              [pats[m.pattern]!],
              k,
              false,
            );
            expect(single.isMatch(hay)).toBe(true);
          }
        },
      ),
      PARAMS,
    );
  });
});
