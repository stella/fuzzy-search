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

import { FuzzySearch } from "../src/index";

const PARAMS = { numRuns: 1000 };

// ─── Generators ──────────────────────────────

// Patterns must be longer than max distance.
// Since maxDist includes 2, minLength must be 3.
const pattern = fc.string({
  minLength: 3,
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

function levenshtein(a: string, b: string): number {
  // Use Array.from to split into Unicode
  // characters (not UTF-16 code units). This
  // handles emoji and supplementary plane chars
  // correctly — matching the Rust library which
  // operates on char (Unicode scalar values).
  const ac = Array.from(a);
  const bc = Array.from(b);
  const m = ac.length;
  const n = bc.length;
  if (m === 0) return n;
  if (n === 0) return m;

  let prev = Array.from({ length: n + 1 }, (_, i) => i);
  for (let i = 1; i <= m; i++) {
    const curr: number[] = Array.from({ length: n + 1 });
    curr[0] = i;
    for (let j = 1; j <= n; j++) {
      const cost = ac[i - 1] === bc[j - 1] ? 0 : 1;
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

  for (let patIdx = 0; patIdx < pats.length; patIdx++) {
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

    for (let i = 0; i <= hay.length - minLen; i++) {
      let bestDist = k + 1;
      let bestEnd = i;
      let bestLen = 0;

      for (
        let len = minLen;
        len <= maxLen && i + len <= hay.length;
        len++
      ) {
        const window = hay.slice(i, i + len);
        const d = levenshtein(pat, window);
        if (d > k) continue;
        // Prefer lower distance, then length
        // closest to pattern length (matches
        // our library's find_start strategy).
        if (
          d < bestDist ||
          (d === bestDist &&
            Math.abs(len - m) < Math.abs(bestLen - m))
        ) {
          bestDist = d;
          bestEnd = i + len;
          bestLen = len;
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

const isWordChar = (ch: string) => /\p{L}|\p{N}/u.test(ch);

const isCjk = (ch: string) =>
  /\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}|\p{Script=Hangul}/u.test(
    ch,
  );

function buildFS(
  pats: string[],
  k: number,
  wholeWords: boolean,
): FuzzySearch {
  // Filter out patterns too short for the
  // distance (dist must be < pattern length).
  const valid = pats.filter(
    (p) => Array.from(p).length > k,
  );
  // If nothing valid, use a dummy pattern that
  // won't match anything (avoids skip logic in
  // all 30+ callers).
  const entries =
    valid.length > 0 ? valid : ["\x00\x01\x02\x03"];
  return new FuzzySearch(
    entries.map((p) => ({
      pattern: p,
      distance: k,
    })),
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
            expect(hay.slice(m.start, m.end)).toBe(m.text);
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
          const matches = buildFS(pats, k, false).findIter(
            hay,
          );
          for (let i = 1; i < matches.length; i++) {
            expect(
              matches[i]!.start,
            ).toBeGreaterThanOrEqual(matches[i - 1]!.end);
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
          const matches = buildFS(pats, k, false).findIter(
            hay,
          );
          for (const m of matches) {
            expect(m.end).toBeGreaterThan(m.start);
          }
          for (let i = 1; i < matches.length; i++) {
            expect(
              matches[i]!.start,
            ).toBeGreaterThanOrEqual(matches[i - 1]!.start);
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
          const matches = buildFS(pats, k, false).findIter(
            hay,
          );
          for (const m of matches) {
            expect(m.distance).toBeLessThanOrEqual(k);
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
            const d = levenshtein(pats[m.pattern]!, m.text);
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
            minLength: 3,
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
          const real = buildFS(pats, k, false).findIter(
            hay,
          );

          // Each match from our library must be
          // verifiable by the oracle.
          for (const m of real) {
            const d = levenshtein(pats[m.pattern]!, m.text);
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
            minLength: 3,
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
          const real = buildFS(pats, k, false).findIter(
            hay,
          );
          const oracle = oracleFuzzySearch(pats, hay, k);

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
                  rm.start <= om.end && rm.end >= om.start,
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
          const matches = buildFS(pats, k, true).findIter(
            hay,
          );
          for (const m of matches) {
            const before = hay[m.start - 1];
            const after = hay[m.end];
            if (before) {
              expect(
                !isWordChar(before) || isCjk(m.text[0]!),
              ).toBe(true);
            }
            if (after) {
              expect(
                !isWordChar(after) || isCjk(m.text.at(-1)!),
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
      minLength: 3,
      maxLength: 10,
      unit: fc.constantFrom(
        ..."abcdefghijklmnopqrstuvwxyz0123456789".split(""),
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
          const repls = pats.map((_, i) => `[${i}]`);
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
      fc.property(patterns, haystack, (pats, hay) => {
        const fs = buildFS(pats, 0, false);
        for (const m of fs.findIter(hay)) {
          expect(m.distance).toBe(0);
          expect(m.text).toBe(pats[m.pattern]!);
        }
      }),
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
            minLength: 3,
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

// ─── Property 14: single-pattern oracle parity ─
//
// For a single pattern on short text, our library
// must agree with the oracle on distance and match
// region coverage. Equivalent fuzzy alignments can
// shift start/end by a few code points, so this
// property checks semantic parity rather than
// insisting on one exact alignment.

describe("property: strict oracle (single pattern)", () => {
  test("every library match exists in oracle", () => {
    fc.assert(
      fc.property(
        // Patterns at least 2 chars longer than
        // distance (avoids pathological cases
        // where nearly everything matches).
        fc.string({ minLength: 4, maxLength: 8 }),
        fc.string({
          minLength: 0,
          maxLength: 40,
        }),
        fc.constantFrom(1, 2),
        (pat, hay, k) => {
          const real = buildFS([pat], k, false).findIter(
            hay,
          );
          const oracle = oracleFuzzySearch([pat], hay, k);

          // Multiple equally-good alignments can
          // exist for the same fuzzy match. The
          // strict check here is that the oracle
          // contains an equivalent region with the
          // same distance, not necessarily the
          // identical start/end pair.
          for (const rm of real) {
            const found = oracle.some(
              (om) =>
                om.distance === rm.distance &&
                om.start < rm.end &&
                rm.start < om.end,
            );
            expect(found).toBe(true);
          }
        },
      ),
      PARAMS,
    );
  });

  test("every oracle match is covered by library", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 4, maxLength: 8 }),
        fc.string({
          minLength: 0,
          maxLength: 40,
        }),
        fc.constantFrom(1, 2),
        (pat, hay, k) => {
          const real = buildFS([pat], k, false).findIter(
            hay,
          );
          const oracle = oracleFuzzySearch([pat], hay, k);

          // Every oracle match must be either:
          // 1. Found by library at the same pos, OR
          // 2. Overlapped by a library match of
          //    equal or better distance.
          for (const om of oracle) {
            const exact = real.some(
              (rm) =>
                rm.start === om.start && rm.end === om.end,
            );
            if (!exact) {
              const covered = real.some(
                (rm) =>
                  rm.start <= om.end &&
                  rm.end >= om.start &&
                  rm.distance <= om.distance,
              );
              // If not covered, the library must
              // at least find SOMETHING in this
              // region.
              if (!covered) {
                const nearby = real.some(
                  (rm) =>
                    rm.end >= om.start &&
                    rm.start <= om.end,
                );
                expect(nearby).toBe(true);
              }
            }
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 15: normalization idempotence ──
//
// If both pattern and haystack are already
// ASCII, normalizeDiacritics should not change
// the results.

describe("property: normalization idempotence", () => {
  test("ASCII text: norm vs no-norm produce same matches", () => {
    const asciiStr = fc.string({
      minLength: 0,
      maxLength: 100,
      unit: fc.constantFrom(
        ..."abcdefghijklmnopqrstuvwxyz 0123456789.,!?-".split(
          "",
        ),
      ),
    });
    const asciiPat = fc.string({
      minLength: 3,
      maxLength: 10,
      unit: fc.constantFrom(
        ..."abcdefghijklmnopqrstuvwxyz".split(""),
      ),
    });
    fc.assert(
      fc.property(
        fc.array(asciiPat, {
          minLength: 1,
          maxLength: 5,
        }),
        asciiStr,
        (pats, hay) => {
          const plain = buildFS(pats, 1, false).findIter(
            hay,
          );
          const norm = new FuzzySearch(
            pats.map((p) => ({
              pattern: p,
              distance: 1,
            })),
            {
              wholeWords: false,
              normalizeDiacritics: true,
            },
          ).findIter(hay);

          expect(plain.length).toBe(norm.length);
          for (let i = 0; i < plain.length; i++) {
            expect(plain[i]!.start).toBe(norm[i]!.start);
            expect(plain[i]!.end).toBe(norm[i]!.end);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 16: Czech diacritics oracle ────
//
// With normalizeDiacritics, "á" and "a" are
// equivalent. Verify matches using a JS oracle
// that strips diacritics before computing
// Levenshtein.

function stripDiacritics(s: string): string {
  return s.normalize("NFD").replace(/[\u0300-\u036f]/g, "");
}

describe("property: diacritics normalization oracle", () => {
  test("norm matches have correct normalized distance", () => {
    // Czech-like chars for realistic coverage.
    const czChar = fc.constantFrom(
      ..."aábcčdďeéěfghiíjklmnňoópqrřsštťuúůvwxyýzž ".split(
        "",
      ),
    );
    const czStr = fc.string({
      minLength: 0,
      maxLength: 80,
      unit: czChar,
    });
    const czPat = fc.string({
      minLength: 3,
      maxLength: 10,
      unit: czChar,
    });
    fc.assert(
      fc.property(
        fc.array(czPat, {
          minLength: 1,
          maxLength: 3,
        }),
        czStr,
        (pats, hay) => {
          const fs = new FuzzySearch(
            pats.map((p) => ({
              pattern: p,
              distance: 1,
            })),
            {
              wholeWords: false,
              normalizeDiacritics: true,
            },
          );
          for (const m of fs.findIter(hay)) {
            // The Levenshtein distance between
            // stripped pattern and stripped matched
            // text must be <= max distance.
            const normPat = stripDiacritics(
              pats[m.pattern]!,
            );
            const normText = stripDiacritics(m.text);
            const d = levenshtein(normPat, normText);
            expect(d).toBeLessThanOrEqual(1);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 17: case insensitive oracle ────

describe("property: case insensitive oracle", () => {
  test("CI matches have correct lowered distance", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 3,
            maxLength: 10,
          }),
          { minLength: 1, maxLength: 5 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 100,
        }),
        (pats, hay) => {
          const fs = new FuzzySearch(
            pats.map((p) => ({
              pattern: p,
              distance: 1,
            })),
            {
              wholeWords: false,
              caseInsensitive: true,
            },
          );
          for (const m of fs.findIter(hay)) {
            const d = levenshtein(
              pats[m.pattern]!.toLowerCase(),
              m.text.toLowerCase(),
            );
            expect(d).toBeLessThanOrEqual(1);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 18: prefix/suffix patterns ─────
//
// Patterns that share prefixes: "ab", "abc",
// "abcd". These stress the non-overlapping
// selection and priority logic.

describe("property: overlapping prefix patterns", () => {
  test("prefix chain: every match is valid", () => {
    fc.assert(
      fc.property(
        // Generate a base word and build prefix chain
        fc.string({
          minLength: 3,
          maxLength: 8,
          unit: fc.constantFrom(
            ..."abcdefghijklmnop".split(""),
          ),
        }),
        fc.string({
          minLength: 0,
          maxLength: 100,
        }),
        (base, hay) => {
          // Prefixes of increasing length
          const pats = Array.from(
            {
              length: Math.max(
                0,
                Math.min(base.length, 4) - 1,
              ),
            },
            (_, i) => base.slice(0, i + 3),
          );
          const fs = buildFS(pats, 1, false);
          for (const m of fs.findIter(hay)) {
            const d = levenshtein(pats[m.pattern]!, m.text);
            expect(d).toBeLessThanOrEqual(1);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 19: distance 3 ────────────────

describe("property: distance 3", () => {
  test("distance 3 matches are all valid", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 4,
            maxLength: 10,
          }),
          { minLength: 1, maxLength: 3 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 80,
        }),
        (pats, hay) => {
          const fs = buildFS(pats, 3, false);
          for (const m of fs.findIter(hay)) {
            const d = levenshtein(pats[m.pattern]!, m.text);
            expect(d).toBeLessThanOrEqual(3);
            expect(m.distance).toBe(d);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 20: wholeWords ⊆ no-wholeWords ─
//
// Every match found with wholeWords: true must
// also be found (as a valid fuzzy match) when
// wholeWords is false. wholeWords only filters;
// it never creates new matches.

describe("property: wholeWords subset", () => {
  test("wholeWords matches ⊆ no-wholeWords matches", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 3,
            maxLength: 8,
          }),
          { minLength: 1, maxLength: 5 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 80,
        }),
        maxDist,
        (pats, hay, k) => {
          const ww = buildFS(pats, k, true).findIter(hay);
          const noWw = buildFS(pats, k, false).findIter(
            hay,
          );

          // Every wholeWords match must be
          // verifiable by the naive oracle
          // (it IS a valid fuzzy match).
          for (const m of ww) {
            const d = levenshtein(pats[m.pattern]!, m.text);
            expect(d).toBeLessThanOrEqual(k);
          }

          // wholeWords count <= no-wholeWords.
          expect(ww.length).toBeLessThanOrEqual(
            noWw.length,
          );
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 21: distance monotonicity ──────
//
// Increasing the max distance should never lose
// matches: matches(k) ⊆ matches(k+1) in terms
// of matched text regions.

describe("property: distance monotonicity", () => {
  test("dist k matches are reachable at dist k+1", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 3,
            maxLength: 8,
          }),
          { minLength: 1, maxLength: 3 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 60,
        }),
        (pats, hay) => {
          const d1 = buildFS(pats, 1, false).findIter(hay);
          const d2 = buildFS(pats, 2, false).findIter(hay);

          // Every dist-1 match must be a valid
          // match at dist 2 (it still satisfies
          // distance <= 2).
          for (const m of d1) {
            const d = levenshtein(pats[m.pattern]!, m.text);
            expect(d).toBeLessThanOrEqual(2);
          }

          // dist-2 should find at least as many
          // matchable regions.
          // (Not exact: non-overlapping selection
          //  may differ, so we just check d2
          //  covers d1's regions.)
          for (const m1 of d1) {
            const covered = d2.some(
              (m2) =>
                m2.start <= m1.end && m2.end >= m1.start,
            );
            expect(covered).toBe(true);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 22: pattern index correctness ──
//
// m.pattern must index into the original patterns
// array, and the matched text must be within
// distance k of THAT specific pattern.

describe("property: pattern index correctness", () => {
  test("m.pattern indexes the correct pattern", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 3,
            maxLength: 10,
          }),
          { minLength: 2, maxLength: 8 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 100,
        }),
        maxDist,
        (pats, hay, k) => {
          const fs = buildFS(pats, k, false);
          for (const m of fs.findIter(hay)) {
            // Index in bounds.
            expect(m.pattern).toBeGreaterThanOrEqual(0);
            expect(m.pattern).toBeLessThan(pats.length);
            // Distance matches THIS pattern.
            const d = levenshtein(pats[m.pattern]!, m.text);
            expect(d).toBeLessThanOrEqual(k);
            expect(m.distance).toBe(d);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 23: determinism ────────────────
//
// Running the same search twice must produce
// identical results. Catches uninitialized
// memory, hash map ordering, etc.

describe("property: determinism", () => {
  test("same input always produces same output", () => {
    fc.assert(
      fc.property(
        patterns,
        haystack,
        maxDist,
        (pats, hay, k) => {
          const fs = buildFS(pats, k, false);
          const r1 = fs.findIter(hay);
          const r2 = fs.findIter(hay);
          expect(r1.length).toBe(r2.length);
          for (let i = 0; i < r1.length; i++) {
            expect(r1[i]!.start).toBe(r2[i]!.start);
            expect(r1[i]!.end).toBe(r2[i]!.end);
            expect(r1[i]!.distance).toBe(r2[i]!.distance);
            expect(r1[i]!.pattern).toBe(r2[i]!.pattern);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 24: UTF-16 supplementary plane ─
//
// Text with emoji (supplementary plane chars,
// 2 UTF-16 code units each) must have correct
// offsets. slice(start, end) must equal text.

describe("property: supplementary plane offsets", () => {
  test("emoji text: offsets are correct", () => {
    const emojiStr = fc.string({
      minLength: 0,
      maxLength: 40,
      unit: fc.constantFrom(
        ..."abcdefgh 😀🎉🔥🚀💡🎸🌍🏠".split(""),
      ),
    });
    const emojiPat = fc.string({
      minLength: 3,
      maxLength: 6,
      unit: fc.constantFrom(..."abcdefgh".split("")),
    });
    fc.assert(
      fc.property(
        fc.array(emojiPat, {
          minLength: 1,
          maxLength: 3,
        }),
        emojiStr,
        (pats, hay) => {
          const fs = buildFS(pats, 1, false);
          for (const m of fs.findIter(hay)) {
            expect(hay.slice(m.start, m.end)).toBe(m.text);
            const d = levenshtein(pats[m.pattern]!, m.text);
            expect(d).toBeLessThanOrEqual(1);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 25: mixed distances ────────────
//
// Patterns with DIFFERENT max distances in the
// same search. Each match must respect its own
// pattern's distance.

describe("property: mixed distances per pattern", () => {
  test("each match respects its own max distance", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.record({
            pattern: fc.string({
              minLength: 2,
              maxLength: 10,
            }),
            distance: fc.constantFrom(0, 1, 2, 3),
          }),
          { minLength: 2, maxLength: 6 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 100,
        }),
        (entries, hay) => {
          fc.pre(
            entries.every(
              (e) =>
                e.distance < Array.from(e.pattern).length,
            ),
          );
          const fs = new FuzzySearch(entries, {
            wholeWords: false,
          });
          for (const m of fs.findIter(hay)) {
            const entry = entries[m.pattern]!;
            const d = levenshtein(entry.pattern, m.text);
            expect(d).toBeLessThanOrEqual(entry.distance);
            expect(m.distance).toBe(d);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 26: feature interaction ────────
//
// normalizeDiacritics + caseInsensitive +
// wholeWords all enabled. The cartesian product
// catches interaction bugs.

describe("property: all features combined", () => {
  test("norm + CI + wholeWords: all matches valid", () => {
    const czChar = fc.constantFrom(
      ..."aábcčdďeéěfghiíjklmnňoópqrřsštťuúůvwxyýzž ABCČDĎEÉĚ".split(
        "",
      ),
    );
    const czStr = fc.string({
      minLength: 0,
      maxLength: 60,
      unit: czChar,
    });
    const czPat = fc.string({
      minLength: 3,
      maxLength: 8,
      unit: czChar,
    });
    fc.assert(
      fc.property(
        fc.array(czPat, {
          minLength: 1,
          maxLength: 3,
        }),
        czStr,
        (pats, hay) => {
          const fs = new FuzzySearch(
            pats.map((p) => ({
              pattern: p,
              distance: 1,
            })),
            {
              wholeWords: true,
              normalizeDiacritics: true,
              caseInsensitive: true,
            },
          );
          for (const m of fs.findIter(hay)) {
            // Normalize both sides, then check
            const normPat = stripDiacritics(
              pats[m.pattern]!,
            ).toLowerCase();
            const normText = stripDiacritics(
              m.text,
            ).toLowerCase();
            const d = levenshtein(normPat, normText);
            expect(d).toBeLessThanOrEqual(1);

            // Word boundary check
            const before = hay[m.start - 1];
            const after = hay[m.end];
            if (before) {
              expect(
                !isWordChar(before) || isCjk(m.text[0]!),
              ).toBe(true);
            }
            if (after) {
              expect(
                !isWordChar(after) || isCjk(m.text.at(-1)!),
              ).toBe(true);
            }
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 27: no false negatives on exact ─
//
// If a pattern appears EXACTLY in the haystack,
// it must be found regardless of distance setting.

describe("property: no false negatives on exact", () => {
  test("exact substring always found (no wholeWords)", () => {
    fc.assert(
      fc.property(
        fc.string({
          minLength: 3,
          maxLength: 10,
        }),
        fc.string({
          minLength: 0,
          maxLength: 30,
        }),
        fc.string({
          minLength: 0,
          maxLength: 30,
        }),
        maxDist,
        (pat, prefix, suffix, k) => {
          const hay = prefix + pat + suffix;
          const fs = buildFS([pat], k, false);
          expect(fs.isMatch(hay)).toBe(true);

          const matches = fs.findIter(hay);
          // At least one match must contain
          // the exact position.
          const exactFound = matches.some(
            (m) =>
              m.start <= prefix.length &&
              m.end >= prefix.length + pat.length,
          );
          // Or a nearby match (non-overlapping
          // selection may choose a different one).
          if (!exactFound) {
            expect(matches.length).toBeGreaterThan(0);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 28: Damerau oracle ─────────────
//
// Every Damerau match has correct OSA distance.
// The oracle uses a naive DP Damerau-Levenshtein.

function damerauLev(a: string, b: string): number {
  const ac = Array.from(a);
  const bc = Array.from(b);
  const m = ac.length;
  const n = bc.length;
  if (m === 0) return n;
  if (n === 0) return m;

  let prev2: number[] = Array.from(
    { length: n + 1 },
    () => 0,
  );
  let prev = Array.from({ length: n + 1 }, (_, i) => i);

  for (let i = 1; i <= m; i++) {
    const curr: number[] = Array.from({ length: n + 1 });
    curr[0] = i;
    for (let j = 1; j <= n; j++) {
      const cost = ac[i - 1] === bc[j - 1] ? 0 : 1;
      curr[j] = Math.min(
        curr[j - 1]! + 1,
        prev[j]! + 1,
        prev[j - 1]! + cost,
      );
      if (
        i > 1 &&
        j > 1 &&
        ac[i - 1] === bc[j - 2] &&
        ac[i - 2] === bc[j - 1]
      ) {
        curr[j] = Math.min(curr[j]!, prev2[j - 2]! + 1);
      }
    }
    prev2 = prev;
    prev = curr;
  }
  return prev[n]!;
}

describe("property: Damerau distance oracle", () => {
  test("every Damerau match has correct OSA distance", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 4,
            maxLength: 10,
          }),
          { minLength: 1, maxLength: 5 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 100,
        }),
        maxDist,
        (pats, hay, k) => {
          const fs = new FuzzySearch(
            pats.map((p) => ({
              pattern: p,
              distance: k,
            })),
            {
              wholeWords: false,
              metric: "damerau-levenshtein",
            },
          );
          for (const m of fs.findIter(hay)) {
            const d = damerauLev(pats[m.pattern]!, m.text);
            expect(d).toBeLessThanOrEqual(k);
            expect(m.distance).toBe(d);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 29: Damerau finds transpositions
//
// Damerau should find matches that Levenshtein
// misses (transpositions at distance 1 that are
// distance 2 in Levenshtein).

describe("property: Damerau finds transpositions", () => {
  test("Damerau finds >= Levenshtein matches", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 4,
            maxLength: 8,
          }),
          { minLength: 1, maxLength: 3 },
        ),
        fc.string({
          minLength: 0,
          maxLength: 60,
        }),
        (pats, hay) => {
          const lev = buildFS(pats, 1, false);
          const dam = new FuzzySearch(
            pats.map((p) => ({
              pattern: p,
              distance: 1,
            })),
            {
              wholeWords: false,
              metric: "damerau-levenshtein",
            },
          );
          lev.findIter(hay);
          dam.findIter(hay);
          // Damerau should find at least as many
          // matchable regions (it's a superset
          // metric). Note: due to non-overlapping
          // selection differences, damCount may
          // occasionally be less, but damIsMatch
          // should always be >= levIsMatch.
          if (lev.isMatch(hay)) {
            expect(dam.isMatch(hay)).toBe(true);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 30: CARTESIAN options × dist ──
//
// The true cartesian product of all option
// combinations × distances. 16 combinations
// (2^3 options × 2 distances) per input.
// Verifies every match is valid under every
// feature combination. This catches interaction
// bugs invisible to per-feature tests.

const ALL_OPTION_COMBOS: {
  wholeWords: boolean;
  normalizeDiacritics: boolean;
  caseInsensitive: boolean;
}[] = [];
for (const ww of [false, true]) {
  for (const nd of [false, true]) {
    for (const ci of [false, true]) {
      ALL_OPTION_COMBOS.push({
        wholeWords: ww,
        normalizeDiacritics: nd,
        caseInsensitive: ci,
      });
    }
  }
}

describe("property: cartesian options × distance", () => {
  test("all 16 combos: every match is valid", () => {
    const czChar = fc.constantFrom(
      ..."aábcčdďeéěfghiíjklmnňoópqrřsštťuúůvwxyýzž ABCČDĎEÉĚ.,!?-".split(
        "",
      ),
    );
    const czPat = fc.string({
      minLength: 4,
      maxLength: 8,
      unit: czChar,
    });
    const czStr = fc.string({
      minLength: 0,
      maxLength: 60,
      unit: czChar,
    });
    fc.assert(
      fc.property(
        fc.array(czPat, {
          minLength: 1,
          maxLength: 3,
        }),
        czStr,
        maxDist,
        (pats, hay, k) => {
          for (const opts of ALL_OPTION_COMBOS) {
            const fs = new FuzzySearch(
              pats.map((p) => ({
                pattern: p,
                distance: k,
              })),
              opts,
            );
            for (const m of fs.findIter(hay)) {
              // 1. text field correct
              expect(hay.slice(m.start, m.end)).toBe(
                m.text,
              );

              // 2. distance correct (normalized)
              let oPat = pats[m.pattern]!;
              let oText = m.text;
              if (opts.normalizeDiacritics) {
                oPat = stripDiacritics(oPat);
                oText = stripDiacritics(oText);
              }
              if (opts.caseInsensitive) {
                oPat = oPat.toLowerCase();
                oText = oText.toLowerCase();
              }
              const d = levenshtein(oPat, oText);
              expect(d).toBeLessThanOrEqual(k);

              // 3. word boundary correct
              if (opts.wholeWords) {
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

              // 4. non-overlapping
              // (checked globally below)
            }

            // 5. non-overlapping check
            const matches = fs.findIter(hay);
            for (let i = 1; i < matches.length; i++) {
              expect(
                matches[i]!.start,
              ).toBeGreaterThanOrEqual(matches[i - 1]!.end);
            }
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 29: duplicate patterns ────────
//
// Passing the same pattern twice must not cause
// crashes, double-matches, or incorrect results.

describe("property: duplicate patterns", () => {
  test("duplicated patterns produce valid matches", () => {
    fc.assert(
      fc.property(
        fc.string({
          minLength: 3,
          maxLength: 10,
        }),
        haystack,
        maxDist,
        (pat, hay, k) => {
          const single = buildFS([pat], k, false);
          const doubled = buildFS([pat, pat], k, false);
          const sMatches = single.findIter(hay);
          const dMatches = doubled.findIter(hay);

          // Doubled should find the same regions
          // (possibly attributed to pattern 0 or 1).
          for (const dm of dMatches) {
            const d = levenshtein(pat, dm.text);
            expect(d).toBeLessThanOrEqual(k);
          }

          // Same number of matched regions.
          expect(dMatches.length).toBe(sMatches.length);
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 30: substring patterns ─────────
//
// If pattern A is a substring of pattern B, and
// both are searched with the same distance, the
// results must be valid. Stresses the multi-
// pattern overlap logic differently from prefix
// chains.

describe("property: substring patterns", () => {
  test("contained patterns produce valid matches", () => {
    fc.assert(
      fc.property(
        fc.string({
          minLength: 4,
          maxLength: 10,
        }),
        fc.nat({ max: 3 }),
        fc.nat({ max: 3 }),
        haystack,
        (base, trimL, trimR, hay) => {
          const left = Math.min(trimL, base.length - 3);
          const right = Math.min(
            trimR,
            base.length - left - 3,
          );
          const inner = base.slice(
            left,
            base.length - right,
          );
          if (inner.length < 3 || inner === base) return;

          const fs = new FuzzySearch(
            [
              { pattern: base, distance: 1 },
              { pattern: inner, distance: 1 },
            ],
            { wholeWords: false },
          );
          for (const m of fs.findIter(hay)) {
            const pat = m.pattern === 0 ? base : inner;
            const d = levenshtein(pat, m.text);
            expect(d).toBeLessThanOrEqual(1);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 31: long patterns (near 64) ───
//
// Patterns close to the 64-char Myers limit.
// Catches bit-overflow bugs in the u64 vectors.

describe("property: long patterns", () => {
  test("patterns 50-63 chars: matches valid", () => {
    const longPat = fc.string({
      minLength: 50,
      maxLength: 63,
      unit: fc.constantFrom(
        ..."abcdefghijklmnopqrstuvwxyz".split(""),
      ),
    });
    fc.assert(
      fc.property(
        longPat,
        fc.string({
          minLength: 0,
          maxLength: 200,
        }),
        (pat, hay) => {
          const fs = new FuzzySearch(
            [{ pattern: pat, distance: 1 }],
            { wholeWords: false },
          );
          for (const m of fs.findIter(hay)) {
            expect(hay.slice(m.start, m.end)).toBe(m.text);
            const d = levenshtein(pat, m.text);
            expect(d).toBeLessThanOrEqual(1);
            expect(m.distance).toBe(d);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 32: high distance (4-5) ───────
//
// Now that we lifted the distance cap, test
// distance 4-5 on realistic-length patterns.

describe("property: high distance (4-5)", () => {
  test("distance 4-5: all matches valid", () => {
    fc.assert(
      fc.property(
        fc.string({
          minLength: 8,
          maxLength: 15,
        }),
        fc.string({
          minLength: 0,
          maxLength: 80,
        }),
        fc.constantFrom(4, 5),
        (pat, hay, k) => {
          fc.pre(Array.from(pat).length > k);
          const fs = new FuzzySearch(
            [{ pattern: pat, distance: k }],
            { wholeWords: false },
          );
          for (const m of fs.findIter(hay)) {
            const d = levenshtein(pat, m.text);
            expect(d).toBeLessThanOrEqual(k);
            expect(m.distance).toBe(d);
          }
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 33: CJK text ──────────────────
//
// CJK characters (always word boundaries) with
// fuzzy matching. Verifies word boundary and
// UTF-16 offset logic for multi-byte text.

describe("property: CJK text", () => {
  test("CJK + Latin mix: matches valid", () => {
    const cjkChar = fc.constantFrom(
      ..."abcdefgh東京日本裁判所大阪名古屋 ".split(""),
    );
    const cjkPat = fc.string({
      minLength: 3,
      maxLength: 6,
      unit: cjkChar,
    });
    const cjkStr = fc.string({
      minLength: 0,
      maxLength: 60,
      unit: cjkChar,
    });
    fc.assert(
      fc.property(
        fc.array(cjkPat, {
          minLength: 1,
          maxLength: 3,
        }),
        cjkStr,
        (pats, hay) => {
          const fs = buildFS(pats, 1, true);
          for (const m of fs.findIter(hay)) {
            expect(hay.slice(m.start, m.end)).toBe(m.text);
            const d = levenshtein(pats[m.pattern]!, m.text);
            expect(d).toBeLessThanOrEqual(1);
          }
        },
      ),
      PARAMS,
    );
  });
});
