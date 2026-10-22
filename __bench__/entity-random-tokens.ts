/** Reproduction benchmark for long EntityIndex tokens over a 16-letter alphabet. */
import { createHash } from "node:crypto";
import { performance } from "node:perf_hooks";

import { EntityIndexBuilder } from "../src/index";
import type { EntityQuery } from "../src/index";

const ENTITY_COUNT = 20_000;
const QUERY_COUNT = 10_000;
const RUNS = 2;
const THRESHOLDS = [0.7999999999999983, 0.9] as const;

const name = (index: number): [string, string] => {
  const hex = createHash("sha256")
    .update(`synthetic-person-${index}`)
    .digest("hex")
    .slice(0, 32);
  const folded = hex
    .split("")
    .map((digit) =>
      String.fromCharCode("a".charCodeAt(0) + Number.parseInt(digit, 16)),
    )
    .join("");
  return [folded.slice(0, 16), folded.slice(16)];
};

const collapseRuns = (text: string) => text.replace(/(.)\1+/gu, "$1");

const query = (index: number): EntityQuery => {
  const words = name(index);
  return {
    tokens: words.map((text) => ({ form: 0, text, weight: 1 })),
    units: words.map((text, position) => ({
      form: 0,
      text,
      positions: [position],
    })),
  };
};

const builder = new EntityIndexBuilder({
  forms: 1,
  budget: { byLength: [0, 0, 0, 1, 1, 2], basis: "shorter" },
  initialWeight: 1,
  initialSimilarity: 0.8,
});
let vocabularySize = 0;
for (let index = 0; index < ENTITY_COUNT; index++) {
  const tokens = [];
  for (const text of name(index)) {
    const id = builder.intern(0, text);
    vocabularySize = Math.max(vocabularySize, id + 1);
    tokens.push({ id, weight: 1, minimumWeight: 0.8 });
  }
  builder.addAlias({
    entryId: index,
    entityGroup: 0,
    quality: 1,
    forms: [{ form: 0, tokens, joins: [], minimumTotal: 1.6 }],
  });
}
const index = builder.finish([new Float64Array(vocabularySize).fill(1)]);
const queries = Array.from({ length: QUERY_COUNT }, (_, offset) => query(offset));
for (const tau of THRESHOLDS) {
  const timings: number[] = [];
  for (let run = 0; run < RUNS; run++) {
    for (const [expected, input] of queries.entries()) {
      const start = performance.now();
      const result = index.search(input, {
        tau,
        quality: 1,
        maxCost: Number.MAX_SAFE_INTEGER,
      });
      timings.push(performance.now() - start);
      if (
        result.exhausted ||
        result.survivors.length !== 1 ||
        result.survivors[0] !== expected
      ) {
        throw new Error(`Unexpected result for query ${expected} at tau ${tau}`);
      }
    }
  }
  timings.sort((left, right) => left - right);
  const median = timings[Math.floor(timings.length / 2)];
  console.log(
    `tau=${tau} queries=${timings.length} median_ms=${median?.toFixed(3)}`,
  );
}


const sixBuilder = new EntityIndexBuilder({
  forms: 2,
  budget: { byLength: [0, 0, 0, 0, 1, 1, 2], basis: "shorter" },
  initialWeight: 1,
  initialSimilarity: 0.8,
});
const sixVocabularySizes = [0, 0];
for (let entity = 0; entity < ENTITY_COUNT; entity++) {
  const raw = name(entity);
  const folded = raw.map(collapseRuns) as [string, string];
  const forms = [folded, raw].map((words, form) => {
    const tokens = words.map((text) => {
      const id = sixBuilder.intern(form, text);
      sixVocabularySizes[form] = Math.max(sixVocabularySizes[form] ?? 0, id + 1);
      return { id, weight: 1, minimumWeight: 0.8 };
    });
    const id = sixBuilder.intern(form, words.join(""));
    sixVocabularySizes[form] = Math.max(sixVocabularySizes[form] ?? 0, id + 1);
    return { form, tokens, joins: [{ id, left: 0, right: 1 }], minimumTotal: 1.6 };
  });
  sixBuilder.addAlias({ entryId: entity, entityGroup: 0, quality: 1, forms });
}
const sixIndex = sixBuilder.finish(
  sixVocabularySizes.map((size) => new Float64Array(size).fill(1)),
);
const sixQueries = Array.from({ length: QUERY_COUNT }, (_, entity) => {
  const raw = name(entity);
  const folded = raw.map(collapseRuns) as [string, string];
  return {
    tokens: folded.map((text) => ({ form: 0, text, weight: 1 })),
    units: [folded, raw].flatMap((words, form) => [
      { form, text: words[0], positions: [0] },
      { form, text: words[1], positions: [1] },
      { form, text: words.join(""), positions: [0, 1] },
    ]),
  } satisfies EntityQuery;
});
const sixTimings: number[] = [];
for (const [expected, input] of sixQueries.entries()) {
  const start = performance.now();
  const result = sixIndex.search(input, {
    tau: 0.7999999999999983,
    quality: 1,
    maxCost: Number.MAX_SAFE_INTEGER,
  });
  sixTimings.push(performance.now() - start);
  if (result.exhausted || result.survivors[0] !== expected) {
    throw new Error(`Unexpected six-unit result for query ${expected}`);
  }
}
sixTimings.sort((left, right) => left - right);
console.log(
  `six_units queries=${sixTimings.length} median_ms=${sixTimings[Math.floor(sixTimings.length / 2)]?.toFixed(3)}`,
);
