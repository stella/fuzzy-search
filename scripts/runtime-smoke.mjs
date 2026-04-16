import assert from "node:assert/strict";

import { FuzzySearch, distance } from "../dist/index.mjs";

const haystack = "Smlouva s Gais1erová v Pribram";
const fuzzy = new FuzzySearch(
  [
    { pattern: "Gaislerová", distance: 1, name: "surname" },
    { pattern: "Příbram", distance: 0, name: "city" },
  ],
  {
    normalizeDiacritics: true,
    wholeWords: true,
  },
);

const matches = fuzzy.findIter(haystack);

assert.equal(fuzzy.patternCount, 2);
assert.equal(fuzzy.isMatch(haystack), true);
assert.equal(matches.length, 2);
assert.equal(matches[0]?.name, "surname");
assert.equal(matches[0]?.text, "Gais1erová");
assert.equal(matches[1]?.name, "city");
assert.equal(matches[1]?.text, "Pribram");
assert.equal(distance("kitten", "sitting"), 3);

const replaced = fuzzy.replaceAll(haystack, [
  "[SURNAME]",
  "[CITY]",
]);
assert.equal(replaced, "Smlouva s [SURNAME] v [CITY]");

console.log("runtime smoke ok");
