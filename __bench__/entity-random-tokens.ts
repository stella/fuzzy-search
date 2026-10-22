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
