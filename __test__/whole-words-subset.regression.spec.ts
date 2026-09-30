import { expect, test } from "bun:test";
import fc from "fast-check";

import { FuzzySearch } from "../src/index";

// Replay the shrunk wholeWords counterexample without a random CI seed.
test("wholeWords subset: replay seed -530956778", () => {
  fc.assert(
    fc.property(
      fc.array(fc.string({ minLength: 3, maxLength: 8 }), {
        minLength: 1,
        maxLength: 5,
      }),
      fc.string({ minLength: 0, maxLength: 80 }),
      fc.constantFrom(1, 2),
      (patterns, haystack, distance) => {
        const entries = patterns.map((pattern) => ({ pattern, distance }));
        const wholeWords = new FuzzySearch(entries, { wholeWords: true }).findIter(haystack);
        const unrestricted = new FuzzySearch(entries, { wholeWords: false }).findIter(haystack);
        expect(wholeWords.length).toBeLessThanOrEqual(unrestricted.length);
      },
    ),
    {
      numRuns: 1000,
      seed: -530956778,
      path: "740:1:3:1:1:1:1:5:3:5:3:3:7:10:0:11:13:4:9:9",
    },
  );
});
