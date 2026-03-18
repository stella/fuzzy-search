/**
 * Property-based tests for @stll/fuzzy-search.
 *
 * Verify algebraic invariants of the API contract
 * using fast-check to generate random inputs.
 *
 * Run manually: bun test __test__/properties.spec.ts
 * NOT run in CI (too slow for the default matrix).
 */
import { describe, expect, test } from "bun:test";
import fc from "fast-check";

import { FuzzySearch } from "../lib";

const PARAMS = { numRuns: 200 };

// Generate non-empty patterns (short, like names)
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

// ─── Property 1: text field correctness ──────

describe("property: text field", () => {
  test("slice(start, end) === text for every match", () => {
    fc.assert(
      fc.property(patterns, haystack, (pats, hay) => {
        const entries = pats.map((p) => ({
          pattern: p,
          distance: 1,
        }));
        const fs = new FuzzySearch(entries, {
          wholeWords: false,
        });
        const matches = fs.findIter(hay);
        for (const m of matches) {
          expect(hay.slice(m.start, m.end)).toBe(
            m.text,
          );
        }
      }),
      PARAMS,
    );
  });
});

// ─── Property 2: non-overlapping ─────────────

describe("property: non-overlapping", () => {
  test("no two matches from same pattern overlap", () => {
    fc.assert(
      fc.property(patterns, haystack, (pats, hay) => {
        const entries = pats.map((p) => ({
          pattern: p,
          distance: 1,
        }));
        const fs = new FuzzySearch(entries, {
          wholeWords: false,
        });
        const matches = fs.findIter(hay);

        // Matches sorted by start; check no
        // overlap within consecutive matches.
        for (let i = 1; i < matches.length; i++) {
          expect(
            matches[i]!.start,
          ).toBeGreaterThanOrEqual(
            matches[i - 1]!.end,
          );
        }
      }),
      PARAMS,
    );
  });
});

// ─── Property 3: monotonic offsets ───────────

describe("property: monotonic offsets", () => {
  test("matches are in ascending start order", () => {
    fc.assert(
      fc.property(patterns, haystack, (pats, hay) => {
        const entries = pats.map((p) => ({
          pattern: p,
          distance: 1,
        }));
        const fs = new FuzzySearch(entries, {
          wholeWords: false,
        });
        const matches = fs.findIter(hay);
        for (let i = 1; i < matches.length; i++) {
          expect(
            matches[i]!.start,
          ).toBeGreaterThanOrEqual(
            matches[i - 1]!.start,
          );
        }
      }),
      PARAMS,
    );
  });

  test("start < end for every match", () => {
    fc.assert(
      fc.property(patterns, haystack, (pats, hay) => {
        const entries = pats.map((p) => ({
          pattern: p,
          distance: 1,
        }));
        const fs = new FuzzySearch(entries, {
          wholeWords: false,
        });
        const matches = fs.findIter(hay);
        for (const m of matches) {
          expect(m.end).toBeGreaterThan(m.start);
        }
      }),
      PARAMS,
    );
  });
});

// ─── Property 4: distance bound ─────────────

describe("property: distance bound", () => {
  test("reported distance <= max_distance", () => {
    fc.assert(
      fc.property(patterns, haystack, (pats, hay) => {
        const entries = pats.map((p) => ({
          pattern: p,
          distance: 1,
        }));
        const fs = new FuzzySearch(entries, {
          wholeWords: false,
        });
        const matches = fs.findIter(hay);
        for (const m of matches) {
          expect(m.distance).toBeLessThanOrEqual(
            1,
          );
        }
      }),
      PARAMS,
    );
  });
});

// ─── Property 5: wholeWords boundaries ───────

const isWordChar = (ch: string) =>
  /\p{L}|\p{N}/u.test(ch);

const isCjk = (ch: string) =>
  /\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}|\p{Script=Hangul}/u.test(
    ch,
  );

describe("property: wholeWords boundaries", () => {
  test("every wholeWords match is at word boundaries", () => {
    fc.assert(
      fc.property(patterns, haystack, (pats, hay) => {
        const entries = pats.map((p) => ({
          pattern: p,
          distance: 1,
        }));
        const fs = new FuzzySearch(entries, {
          wholeWords: true,
        });
        const matches = fs.findIter(hay);
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
      }),
      PARAMS,
    );
  });
});

// ─── Property 6: exact match found ──────────
//
// If the pattern appears literally in the text
// surrounded by spaces, it MUST be found.

describe("property: exact match always found", () => {
  test("pattern surrounded by spaces is found", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({
            minLength: 1,
            maxLength: 10,
          }),
          { minLength: 1, maxLength: 10 },
        ),
        fc.nat(),
        (pats, idx) => {
          const uniquePats = [...new Set(pats)];
          if (uniquePats.length === 0) return;
          const target =
            uniquePats[idx % uniquePats.length]!;

          const hay = `xxx ${target} yyy`;
          const entries = uniquePats.map((p) => ({
            pattern: p,
            distance: 1,
          }));
          const fs = new FuzzySearch(entries, {
            wholeWords: true,
          });
          const matches = fs.findIter(hay);

          // The target must be found.
          const found = matches.some(
            (m) =>
              m.start >= 4 &&
              m.end <= 4 + target.length + 1,
          );
          expect(found).toBe(true);
        },
      ),
      PARAMS,
    );
  });
});

// ─── Property 7: replaceAll consistency ──────

describe("property: replaceAll consistency", () => {
  test("replaceAll matches findIter-based reconstruction", () => {
    fc.assert(
      fc.property(patterns, haystack, (pats, hay) => {
        const entries = pats.map((p) => ({
          pattern: p,
          distance: 1,
        }));
        const fs = new FuzzySearch(entries, {
          wholeWords: false,
        });
        const matches = fs.findIter(hay);
        const repls = pats.map((_, i) => `[${i}]`);
        const result = fs.replaceAll(hay, repls);

        // Manually build expected.
        let expected = "";
        let last = 0;
        for (const m of matches) {
          expected += hay.slice(last, m.start);
          expected += repls[m.pattern]!;
          last = m.end;
        }
        expected += hay.slice(last);

        expect(result).toBe(expected);
      }),
      PARAMS,
    );
  });
});

// ─── Property 8: isMatch ↔ findIter ─────────

describe("property: isMatch agrees with findIter", () => {
  test("isMatch returns true iff findIter has results", () => {
    fc.assert(
      fc.property(patterns, haystack, (pats, hay) => {
        const entries = pats.map((p) => ({
          pattern: p,
          distance: 1,
        }));
        const fs = new FuzzySearch(entries, {
          wholeWords: true,
        });
        expect(fs.isMatch(hay)).toBe(
          fs.findIter(hay).length > 0,
        );
      }),
      PARAMS,
    );
  });
});
