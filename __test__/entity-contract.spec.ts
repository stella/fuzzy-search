import { afterEach, describe, expect, test } from "bun:test";

import {
  type EntityAliasInput,
  EntityIndex,
  EntityIndexBuilder,
  initBinding,
  type NativeBinding,
} from "../src/core";

type IndexBackend = ReturnType<
  InstanceType<NativeBinding["EntityIndexBuilder"]>["finish"]
>;
type PackedResult = ReturnType<IndexBackend["_searchPacked"]>;

const packedResult = () =>
  ({
    matchKeys: new Uint32Array([0, 1, 2]),
    similarities: new Float64Array([0.8]),
    queryWeights: new Float64Array([3]),
    survivors: new Uint32Array([4]),
    cost: new Float64Array([5, 6, 7, 8, 9]),
    exhausted: false,
  }) satisfies PackedResult;

const restores: (() => void)[] = [];
afterEach(() => {
  for (const restore of restores.reverse()) restore();
  restores.length = 0;
});

const bindResult = (result: PackedResult) => {
  const index = {
    nativeBytes: 0,
    postingCount: 0,
    toBytes: () => new Uint8Array(),
    _searchPacked: () => result,
  } satisfies IndexBackend;
  restores.push(
    initBinding({
      FuzzySearch: class {
        patternCount = 0;
        isMatch() {
          return false;
        }
        _findIterPacked() {
          return new Uint32Array();
        }
        replaceAll() {
          return "";
        }
      },
      distance: () => 0,
      EntityIndex: { fromBytes: () => index },
      EntityIndexBuilder: class {
        intern() {
          return 0;
        }
        addAlias() {
          return 0;
        }
        finishStep() {
          return true;
        }
        finish() {
          return index;
        }
        async finishAsync() {
          return index;
        }
      },
    } satisfies NativeBinding),
  );
  return EntityIndex.fromBytes(new Uint8Array());
};

const query = { tokens: [], units: [] };
const params = { tau: 0, quality: 1, maxCost: 100 };

describe("entity packed backend contract", () => {
  test("preserves typed arrays and maps every cost position", () => {
    const packed = packedResult();
    const result = bindResult(packed).search(query, params);
    expect(result.matchKeys).toBe(packed.matchKeys);
    expect(result.similarities).toBe(packed.similarities);
    expect(result.queryWeights).toBe(packed.queryWeights);
    expect(result.survivors).toBe(packed.survivors);
    expect(result.cost).toEqual({
      lexiconCells: 5,
      lexiconMatches: 6,
      sortedAccesses: 7,
      randomAccessPairs: 8,
      survivors: 9,
    });
  });

  test("rejects every malformed packed dimension", () => {
    for (const length of [0, 1, 2, 4, 5, 6]) {
      const packed = packedResult();
      packed.matchKeys = new Uint32Array(length);
      expect(() => bindResult(packed).search(query, params)).toThrow(
        "Malformed packed entity result",
      );
    }
    for (const length of [0, 1, 2, 3, 4, 6]) {
      const packed = packedResult();
      packed.cost = new Float64Array(length);
      expect(() => bindResult(packed).search(query, params)).toThrow(
        "Malformed packed entity result",
      );
    }
  });
});

const builderOptions = {
  forms: 1,
  budget: { byLength: [0], basis: "shorter" },
  initialWeight: 1,
  initialSimilarity: 0.8,
} satisfies ConstructorParameters<typeof EntityIndexBuilder>[0];

const aliasInput = () =>
  ({
    entryId: 0,
    entityGroup: 0,
    quality: 1,
    forms: [
      {
        form: 0,
        tokens: [{ id: 0, weight: 1, minimumWeight: 1 }],
        joins: [{ id: 0, left: 0, right: 0 }],
        minimumTotal: 1,
      },
    ],
  }) satisfies EntityAliasInput;

test("entity unsigned inputs reject values NAPI would coerce", () => {
  const index = bindResult(packedResult());
  const builder = new EntityIndexBuilder(builderOptions);
  for (const value of [
    -1,
    0.5,
    0x1_0000_0000,
    Number.NaN,
    Number.POSITIVE_INFINITY,
  ]) {
    expect(
      () =>
        new EntityIndexBuilder({
          ...builderOptions,
          forms: value,
        }),
    ).toThrow(RangeError);
    expect(() => builder.intern(value, "alpha")).toThrow(RangeError);
    expect(() => builder.finishStep(value)).toThrow(RangeError);
    for (const field of ["entryId", "entityGroup"] as const) {
      const alias = aliasInput();
      alias[field] = value;
      expect(() => builder.addAlias(alias)).toThrow(RangeError);
    }
    for (const alias of [
      {
        ...aliasInput(),
        forms: [
          {
            form: value,
            tokens: [],
            joins: [],
            minimumTotal: 1,
          },
        ],
      },
      {
        ...aliasInput(),
        forms: [
          {
            form: 0,
            tokens: [{ id: value, weight: 1, minimumWeight: 1 }],
            joins: [],
            minimumTotal: 1,
          },
        ],
      },
      ...(["id", "left", "right"] as const).map((field) => {
        const changed = aliasInput();
        for (const form of changed.forms) {
          for (const join of form.joins) join[field] = value;
        }
        return changed;
      }),
    ])
      expect(() => builder.addAlias(alias)).toThrow(RangeError);
    expect(() =>
      index.search(
        {
          tokens: [{ form: value, text: "alpha", weight: 1 }],
          units: [],
        },
        params,
      ),
    ).toThrow(RangeError);
    expect(() =>
      index.search(
        {
          tokens: [],
          units: [{ form: value, text: "alpha", positions: [] }],
        },
        params,
      ),
    ).toThrow(RangeError);
    expect(() =>
      index.search(
        {
          tokens: [],
          units: [{ form: 0, text: "alpha", positions: [value] }],
        },
        params,
      ),
    ).toThrow(RangeError);
  }
  for (const value of [-1, 0.5, 256, Number.NaN, Number.POSITIVE_INFINITY]) {
    expect(
      () =>
        new EntityIndexBuilder({
          ...builderOptions,
          budget: { byLength: [value], basis: "shorter" },
        }),
    ).toThrow(RangeError);
  }
  expect(() => builder.finishStep(0)).toThrow(RangeError);
});
