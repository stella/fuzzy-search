import { expect, test } from "bun:test";

import { FuzzySearch } from "../src/index";

// Seed -530956778, path 740:1:3:1:1:1:1:5:3:5:3:3:7:10:0:11:13:4:9:9.
// Literal whitespace is a valid pattern. Boundary filtering precedes greedy
// selection, so unrestricted selection can suppress both whole-word windows.
test("wholeWords preserves valid windows displaced by unrestricted selection", () => {
  const entries = [{ pattern: "   ", distance: 2 }];
  const haystack = "0  0";
  const wholeWords = new FuzzySearch(entries, {
    wholeWords: true,
  });
  const unrestricted = new FuzzySearch(entries, {
    wholeWords: false,
  });
  const spans = (search: FuzzySearch) =>
    search
      .findIter(haystack)
      .map(({ start, end, text, distance }) => ({
        start,
        end,
        text,
        distance,
      }));

  expect(spans(wholeWords)).toEqual([
    { start: 0, end: 2, text: "0 ", distance: 2 },
    { start: 2, end: 4, text: " 0", distance: 2 },
  ]);
  expect(spans(unrestricted)).toEqual([
    { start: 0, end: 3, text: "0  ", distance: 1 },
  ]);
});
