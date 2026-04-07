import assert from "node:assert/strict";

const mod = await import(
  new URL("../dist/index.mjs", import.meta.url)
);

assert.equal(typeof mod.FuzzySearch, "function");
assert.equal(typeof mod.distance, "function");

const fs = new mod.FuzzySearch(
  [{ pattern: "hello", distance: 1 }],
  { wholeWords: false },
);
const matches = fs.findIter("say helo world");

assert.equal(matches.length, 1);
assert.equal(matches[0]?.text, "helo");
assert.equal(matches[0]?.distance, 1);
assert.equal(mod.distance("kitten", "sitting"), 3);

process.stdout.write("Node ESM smoke test passed\n");
