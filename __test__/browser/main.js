// Runs the packed @stll/fuzzy-search-wasm package in the browser and writes
// the outcome into #out for check.mjs.
const out = document.getElementById("out");

const assertEqual = (label, actual, expected) => {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${label}: expected ${expectedJson}, got ${actualJson}`);
  }
};

const entityOptions = {
  forms: 1,
  budget: { byLength: [0, 0, 0, 0, 1, 1, 2], basis: "shorter" },
  initialWeight: 1,
  initialSimilarity: 0.8,
};

const buildEntityIndex = (EntityIndexBuilder) => {
  const builder = new EntityIndexBuilder(entityOptions);
  for (const [entryId, text] of ["Gaislerová", "Příbram"].entries()) {
    const id = builder.intern(0, text);
    builder.addAlias({
      entryId,
      entityGroup: entryId,
      quality: 1,
      forms: [
        {
          form: 0,
          tokens: [{ id, weight: 1, minimumWeight: 1 }],
          joins: [],
          minimumTotal: 1,
        },
      ],
    });
  }
  return builder;
};

try {
  const { EntityIndexBuilder, FuzzySearch, distance } =
    await import("@stll/fuzzy-search-wasm");

  // Diacritic-insensitive matching with a typo ("1" for "l").
  const haystack = "Smlouva s Gais1erova v Pribram";
  const search = new FuzzySearch(
    [
      { pattern: "Gaislerová", distance: 1 },
      { pattern: "Novák", distance: 1 },
      { pattern: "Příbram", distance: 2 },
    ],
    { normalizeDiacritics: true, wholeWords: true },
  );
  const found = search.findIter(haystack);
  for (const match of found) {
    assertEqual(
      "match offsets",
      haystack.slice(match.start, match.end),
      match.text,
    );
  }
  const matches = found.map(({ pattern, text, distance: edits }) => ({
    pattern,
    text,
    distance: edits,
  }));
  assertEqual("findIter", matches, [
    { pattern: 0, text: "Gais1erova", distance: 1 },
    { pattern: 2, text: "Pribram", distance: 0 },
  ]);

  const kitten = distance("kitten", "sitting");
  assertEqual("distance", kitten, 3);

  // finishAsync builds on the WASI worker pool, so it needs working workers.
  const weights = () => [new Float64Array(2).fill(1)];
  const syncIndex = buildEntityIndex(EntityIndexBuilder).finish(weights());
  const asyncIndex =
    await buildEntityIndex(EntityIndexBuilder).finishAsync(weights());
  const syncBytes = [...syncIndex.toBytes()];
  assertEqual("finishAsync bytes", [...asyncIndex.toBytes()], syncBytes);

  out.textContent = JSON.stringify({
    ok: true,
    crossOriginIsolated: globalThis.crossOriginIsolated,
    matches,
    distance: kitten,
    entityIndexBytes: syncBytes.length,
  });
} catch (error) {
  out.textContent = JSON.stringify({
    ok: false,
    error: String(error?.stack ?? error),
  });
}
