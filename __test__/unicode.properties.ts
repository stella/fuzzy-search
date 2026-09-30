import { describe, expect, test } from "bun:test";
import fc from "fast-check";

import { distance, FuzzySearch } from "../src/index";
import type { FuzzyMatch } from "../src/index";
import { assertProperty } from "./property-testing";

const scalar = fc.constantFrom(
  "a",
  "b",
  "c",
  "é",
  "ř",
  "\u0301",
  "\u030c",
  "😀",
  "𐐀",
  "\u00a0",
  " ",
);
const text = fc
  .array(scalar, { maxLength: 14 })
  .map((chars) => chars.join(""));
const pattern = fc
  .array(scalar, { minLength: 3, maxLength: 8 })
  .map((chars) => chars.join(""));

/** Exhaust all scalar-aligned windows; no Myers or greedy selection involved. */
const candidates = (
  needle: string,
  haystack: string,
  budget: number,
) => {
  const chars = Array.from(haystack);
  const offsets = [0];
  for (const char of chars)
    offsets.push(offsets.at(-1)! + char.length);
  const windows: {
    start: number;
    end: number;
    distance: number;
  }[] = [];
  for (let start = 0; start < chars.length; start++) {
    for (let end = start + 1; end <= chars.length; end++) {
      const window = chars.slice(start, end).join("");
      const d = referenceDistance(needle, window);
      if (d <= budget)
        windows.push({
          start: offsets[start]!,
          end: offsets[end]!,
          distance: d,
        });
    }
  }
  return windows;
};

const referenceDistance = (
  left: string,
  right: string,
): number => {
  const a = Array.from(left);
  const b = Array.from(right);
  let row = Array.from(
    { length: b.length + 1 },
    (_, i) => i,
  );
  for (const [i, char] of a.entries()) {
    const next = [i + 1];
    for (const [j, other] of b.entries()) {
      next.push(
        Math.min(
          next[j]! + 1,
          row[j + 1]! + 1,
          row[j]! + Number(char !== other),
        ),
      );
    }
    row = next;
  }
  return row.at(-1)!;
};

const assertSpans = (
  matches: FuzzyMatch[],
  haystack: string,
): void => {
  let previousEnd = 0;
  const boundaries = new Set([0]);
  let offset = 0;
  for (const char of haystack) {
    offset += char.length;
    boundaries.add(offset);
  }
  for (const match of matches) {
    expect(match.start).toBeGreaterThanOrEqual(previousEnd);
    expect(match.end).toBeGreaterThan(match.start);
    expect(match.end).toBeLessThanOrEqual(haystack.length);
    expect(boundaries.has(match.start)).toBe(true);
    expect(boundaries.has(match.end)).toBe(true);
    expect(haystack.slice(match.start, match.end)).toBe(
      match.text,
    );
    previousEnd = match.end;
  }
};

describe("Unicode matching contracts", () => {
  test("distance laws hold over Unicode scalars", () => {
    assertProperty(
      "unicode-distance-laws",
      fc.property(text, text, text, (a, b, c) => {
        for (const metric of [
          "levenshtein",
          "damerau-levenshtein",
        ] as const) {
          expect(distance(a, a, metric)).toBe(0);
          expect(distance(a, b, metric)).toBe(
            distance(b, a, metric),
          );
          expect(
            distance(a, b, metric),
          ).toBeGreaterThanOrEqual(
            Math.abs(
              Array.from(a).length - Array.from(b).length,
            ),
          );
        }
        expect(distance(a, b)).toBe(
          referenceDistance(a, b),
        );
        // Restricted Damerau (OSA) does not claim the triangle inequality.
        expect(distance(a, c)).toBeLessThanOrEqual(
          distance(a, b) + distance(b, c),
        );
      }),
    );
  });

  test("small searches agree with exhaustive candidate windows", () => {
    assertProperty(
      "unicode-candidate-reference",
      fc.property(
        pattern,
        text,
        fc.boolean(),
        fc.integer({ min: 0, max: 1 }),
        (needle, drawn, embed, budget) => {
          const haystack = embed
            ? `${drawn}\u00a0${needle}`
            : drawn;
          const expected = candidates(
            needle,
            haystack,
            budget,
          );
          const search = new FuzzySearch(
            [{ pattern: needle, distance: budget }],
            { wholeWords: false },
          );
          const found = search.findIter(haystack);
          expect(found.length > 0).toBe(
            expected.length > 0,
          );
          expect(search.isMatch(haystack)).toBe(
            expected.length > 0,
          );
          for (const { start, end, distance: d } of found) {
            expect(expected).toContainEqual({
              start,
              end,
              distance: d,
            });
          }
          assertSpans(found, haystack);
        },
      ),
      { numRuns: 100 },
    );
  });

  test("exact searches return the brute-force non-overlapping spans", () => {
    assertProperty(
      "unicode-exact-reference",
      fc.property(pattern, text, (needle, drawn) => {
        const haystack = `${drawn}\u00a0${needle}\u00a0${needle}`;
        const expected = candidates(needle, haystack, 0);
        const selected: typeof expected = [];
        let previousEnd = 0;
        for (const candidate of expected) {
          if (candidate.start < previousEnd) continue;
          selected.push(candidate);
          previousEnd = candidate.end;
        }
        const found = new FuzzySearch(
          [{ pattern: needle, distance: 0 }],
          { wholeWords: false },
        ).findIter(haystack);
        expect(
          found.map(({ start, end, distance: d }) => ({
            start,
            end,
            distance: d,
          })),
        ).toEqual(selected);
        assertSpans(found, haystack);
      }),
      { numRuns: 100 },
    );
  });

  test("enabled folding preserves the canonical match sequence", () => {
    const letters = fc
      .array(fc.constantFrom("a", "b", "é", "ř", "ñ"), {
        minLength: 3,
        maxLength: 8,
      })
      .map((chars) => chars.join(""));
    assertProperty(
      "unicode-folding",
      fc.property(
        letters,
        fc.boolean(),
        fc.boolean(),
        (needle, caseInsensitive, normalizeDiacritics) => {
          const fold = (value: string) => {
            const cased = caseInsensitive
              ? value.toLowerCase()
              : value;
            return normalizeDiacritics
              ? cased
                  .normalize("NFD")
                  .replace(/\p{M}/gu, "")
              : cased;
          };
          const spelled = caseInsensitive
            ? needle.toUpperCase()
            : needle;
          const haystack = `😀\u00a0${normalizeDiacritics ? spelled.normalize("NFD") : spelled}\u00a0${spelled}`;
          const actual = new FuzzySearch(
            [{ pattern: needle, distance: 0 }],
            {
              wholeWords: true,
              caseInsensitive,
              normalizeDiacritics,
            },
          ).findIter(haystack);
          const canonical = new FuzzySearch(
            [{ pattern: fold(needle), distance: 0 }],
            { wholeWords: true },
          ).findIter(fold(haystack));
          expect(
            actual.map((match) => ({
              text: fold(match.text),
              distance: match.distance,
            })),
          ).toEqual(
            canonical.map(({ text, distance: d }) => ({
              text,
              distance: d,
            })),
          );
          expect(actual.length).toBe(2);
          assertSpans(actual, haystack);
        },
      ),
    );
  });

  test("adversarial long inputs stay within the work budget", () => {
    assertProperty(
      "unicode-work-budget",
      fc.property(
        fc.constantFrom("a", "\u0301", "😀", "\u00a0"),
        fc.integer({ min: 16_000, max: 32_000 }),
        (unit, count) => {
          const haystack = `${unit.repeat(count)}\u00a0needle\u00a0`;
          const started = performance.now();
          const found = new FuzzySearch(
            [{ pattern: "needle", distance: 1 }],
            { wholeWords: false },
          ).findIter(haystack);
          expect(
            found.some(({ text }) => text === "needle"),
          ).toBe(true);
          expect(performance.now() - started).toBeLessThan(
            2000,
          );
          assertSpans(found, haystack);
        },
      ),
      { numRuns: 8 },
    );
  });
});
