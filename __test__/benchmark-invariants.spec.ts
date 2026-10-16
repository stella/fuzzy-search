import { expect, test } from "bun:test";

import {
  requiredAt,
  speedupRows,
} from "../__bench__/required-at";

test("benchmark lookups preserve every populated index, including falsy values", () => {
  const values = [
    0,
    1,
    -1,
    Number.NaN,
    false,
    "",
    "text",
    null,
  ];
  for (const [index, value] of values.entries()) {
    expect(requiredAt(values, index)).toBe(value);
  }
});

test("benchmark lookups fail for every absent index instead of producing a result", () => {
  const values = [1, 2, 3];
  for (const index of [
    -1,
    3,
    4,
    0.5,
    Number.NaN,
    Infinity,
  ]) {
    expect(() => requiredAt(values, index)).toThrow(
      RangeError,
    );
  }
  expect(() => requiredAt([], 0)).toThrow(RangeError);
  expect(() =>
    requiredAt(Array.from({ length: 3 }), 1),
  ).toThrow(RangeError);
  expect(() => requiredAt([undefined], 0)).toThrow(
    RangeError,
  );
});

test("skipped comparators retain timing-to-name correspondence", () => {
  expect(
    speedupRows(
      ["baseline", "skipped", "measured"],
      [2, Number.NaN, 6],
    ),
  ).toEqual([{ name: "measured", multiple: 3 }]);
  expect(speedupRows(["baseline"], [2])).toEqual([]);
  expect(() => speedupRows([], [])).toThrow(RangeError);
  expect(() =>
    speedupRows(["baseline", "missing"], [2]),
  ).toThrow(RangeError);
  expect(() => speedupRows(["baseline"], [2, 6])).toThrow(
    RangeError,
  );
});

test("speedup baselines must be positive and finite", () => {
  for (const baseline of [
    -Infinity,
    -1,
    -Number.MIN_VALUE,
    -0,
    0,
    Number.NaN,
    Infinity,
  ]) {
    expect(() =>
      speedupRows(["baseline", "measured"], [baseline, 6]),
    ).toThrow(
      "Benchmark baseline must be positive and finite",
    );
  }
  for (const baseline of [
    Number.MIN_VALUE,
    0.125,
    1,
    Number.MAX_VALUE,
  ]) {
    expect(
      speedupRows(
        ["baseline", "measured"],
        [baseline, baseline],
      ),
    ).toEqual([{ name: "measured", multiple: 1 }]);
  }
});
