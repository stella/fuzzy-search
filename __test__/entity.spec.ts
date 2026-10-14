import { describe, expect, test } from "bun:test";

import {
  EntityIndex,
  EntityIndexBuilder,
  type EntityIndexOptions,
  type EntityQuery,
} from "../src/index";

const options = {
  forms: 2,
  budget: {
    byLength: [0, 0, 0, 0, 1, 1, 2],
    basis: "shorter",
  },
  initialWeight: 1,
  initialSimilarity: 0.8,
} satisfies EntityIndexOptions;

const fixture = () => {
  const builder = new EntityIndexBuilder(options);
  const spellings = [
    "alpha",
    "álpha",
    "東京会社",
    "😀abcd",
  ];
  for (let form = 0; form < options.forms; form++) {
    for (const [entryId, text] of spellings.entries()) {
      const id = builder.intern(form, text);
      expect(builder.intern(form, text)).toBe(id);
      builder.addAlias({
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
  return {
    builder,
    weights: [
      new Float64Array(4).fill(1),
      new Float64Array(4).fill(1),
    ],
  };
};

const query = (text: string, form = 0) =>
  ({
    tokens: [{ form, text, weight: 1 }],
    units: [{ form, text, positions: [0] }],
  }) satisfies EntityQuery;
const params = { tau: 0.7, quality: 1, maxCost: 1_000_000 };

// One contract covers the wrapper and the generated NAPI backend. The WASM
// entry point exports the same classes through this shared wrapper.
describe("EntityIndex", () => {
  test("packed Unicode hits and counters survive serialization", () => {
    const { builder, weights } = fixture();
    const index = builder.finish(weights);
    const restored = EntityIndex.fromBytes(index.toBytes());
    expect(index.nativeBytes).toBeGreaterThan(0);
    expect(index.postingCount).toBeGreaterThan(0);
    for (const { text, ids, similarities } of [
      {
        text: "alpha",
        ids: [0, 1],
        similarities: [1, 0.8],
      },
      { text: "alhpa", ids: [0], similarities: [0.8] },
      { text: "東京会社", ids: [2], similarities: [1] },
      { text: "😀abdc", ids: [3], similarities: [0.8] },
    ]) {
      for (const form of [0, 1]) {
        const result = index.search(
          query(text, form),
          params,
        );
        expect(
          restored.search(query(text, form), params),
        ).toEqual(result);
        expect(result.matchKeys).toBeInstanceOf(
          Uint32Array,
        );
        expect(result.similarities).toBeInstanceOf(
          Float64Array,
        );
        expect(result.queryWeights).toBeInstanceOf(
          Float64Array,
        );
        expect(result.survivors).toBeInstanceOf(
          Uint32Array,
        );
        expect(result.matchKeys.length).toBe(
          result.similarities.length * 3,
        );
        expect(Array.from(result.matchKeys)).toEqual(
          ids.flatMap((id) => [0, form, id]),
        );
        expect(Array.from(result.similarities)).toEqual(
          similarities,
        );
        expect(Array.from(result.survivors)).toEqual(
          ids.map((id) => id + form * 4),
        );
        expect(result.exhausted).toBe(false);
        for (const value of Object.values(result.cost)) {
          expect(Number.isSafeInteger(value)).toBe(true);
          expect(value).toBeGreaterThanOrEqual(0);
        }
      }
    }
    expect(() => builder.intern(0, "later")).toThrow(
      "consumed",
    );
  });

  test("async finish transfers ownership immediately and matches sync finish", async () => {
    const first = fixture();
    const second = fixture();
    const expected = first.builder.finish(first.weights);
    const pending = second.builder.finishAsync(
      second.weights,
    );
    expect(() => second.builder.intern(0, "later")).toThrow(
      "consumed",
    );
    for (const weights of second.weights) weights.fill(99);
    const actual = await pending;
    expect(actual.toBytes()).toEqual(expected.toBytes());
    expect(actual.search(query("alhpa"), params)).toEqual(
      expected.search(query("alhpa"), params),
    );
  });

  test("stepped finish produces the same index", () => {
    const first = fixture();
    const second = fixture();
    const expected = first.builder.finish(first.weights);
    const steps = second.builder.finishSteps(
      second.weights,
      1,
    );
    let step = steps.next();
    while (!step.done) step = steps.next();
    expect(step.value.toBytes()).toEqual(
      expected.toBytes(),
    );
  });

  test("rejects invalid bytes and unsafe work budgets", () => {
    expect(() =>
      EntityIndex.fromBytes(new Uint8Array([1, 2, 3])),
    ).toThrow();
    const { builder, weights } = fixture();
    const index = builder.finish(weights);
    for (const maxCost of [
      -1,
      1.5,
      Number.NaN,
      Number.POSITIVE_INFINITY,
      Number.MAX_SAFE_INTEGER + 1,
    ]) {
      expect(() =>
        index.search(query("alpha"), {
          ...params,
          maxCost,
        }),
      ).toThrow("safe integer");
    }
    expect(
      index.search(query("alpha"), {
        ...params,
        maxCost: 0,
      }).exhausted,
    ).toBe(true);
  });
});
