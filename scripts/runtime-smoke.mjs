import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import {
  EntityIndex,
  EntityIndexBuilder,
  FuzzySearch,
  distance,
} from "../dist/index.mjs";

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

const entityBuilder = new EntityIndexBuilder({
  forms: 2,
  budget: {
    byLength: [0, 0, 0, 0, 1, 1, 2],
    basis: "shorter",
  },
  initialWeight: 1,
  initialSimilarity: 0.8,
});
for (const form of [0, 1]) {
  for (const [entryId, text] of [
    "alpha",
    "álpha",
    "東京会社",
    "😀abcd",
  ].entries()) {
    const id = entityBuilder.intern(form, text);
    entityBuilder.addAlias({
      entryId,
      entityGroup: entryId,
      quality: 1,
      forms: [
        {
          form,
          tokens: [{ id, weight: 1, minimumWeight: 1 }],
          joins: [],
          minimumTotal: 1,
        },
      ],
    });
  }
}
const entityIndex = await entityBuilder.finishAsync([
  new Float64Array(4).fill(1),
  new Float64Array(4).fill(1),
]);
assert.throws(
  () => entityBuilder.intern(0, "later"),
  /consumed/,
);
const entityRestored = EntityIndex.fromBytes(
  entityIndex.toBytes(),
);
for (const { text, ids, similarities } of [
  { text: "alpha", ids: [0, 1], similarities: [1, 0.8] },
  { text: "alhpa", ids: [0], similarities: [0.8] },
  { text: "東京会社", ids: [2], similarities: [1] },
  { text: "😀abdc", ids: [3], similarities: [0.8] },
]) {
  for (const form of [0, 1]) {
    const query = {
      tokens: [{ form, text, weight: 1 }],
      units: [{ form, text, positions: [0] }],
    };
    const params = {
      tau: 0.7,
      quality: 1,
      maxCost: 1_000_000,
    };
    const result = entityIndex.search(query, params);
    assert.deepEqual(
      Array.from(result.matchKeys),
      ids.flatMap((id) => [0, form, id]),
    );
    assert.deepEqual(
      Array.from(result.similarities),
      similarities,
    );
    assert.deepEqual(
      Array.from(result.survivors),
      ids.map((id) => id + form * 4),
    );
    assert.equal(result.exhausted, false);
    assert.deepEqual(
      entityRestored.search(query, params),
      result,
    );
  }
}

// build:js provides the WASM artifact in runtime CI. Force the loader's strict
// WASI mode in a fresh Node process so a native fallback cannot pass this check.
if (process.env.NAPI_RS_FORCE_WASI !== "error") {
  const wasi = spawnSync(
    "node",
    [fileURLToPath(import.meta.url)],
    {
      env: { ...process.env, NAPI_RS_FORCE_WASI: "error" },
      stdio: "inherit",
      timeout: 30_000,
    },
  );
  if (wasi.error) throw wasi.error;
  assert.equal(wasi.status, 0, "WASI runtime smoke failed");
}

console.log("runtime smoke ok");
