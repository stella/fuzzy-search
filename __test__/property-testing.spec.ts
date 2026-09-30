import { expect, test } from "bun:test";
import fc from "fast-check";

import {
  assertProperty,
  propertyConfig,
  PropertyTestConfigError,
} from "./property-testing";

test("property budgets scale without changing the generated seed", () => {
  assertProperty(
    "configuration-budget",
    fc.property(
      fc.integer({ min: 1, max: 50 }),
      fc.integer({ min: 1, max: 20 }),
      (numRuns, factor) => {
        const base = propertyConfig({
          params: { numRuns },
          env: {},
        });
        const expanded = propertyConfig({
          params: { numRuns },
          env: {
            PROPERTY_TEST_NUM_RUNS_FACTOR: String(factor),
          },
        });
        expect(expanded.numRuns).toBe(numRuns * factor);
        expect(expanded.seed).toBe(base.seed);
      },
    ),
  );
});

test("explicit replay configuration is preserved", () => {
  assertProperty(
    "configuration-replay",
    fc.property(
      fc.integer(),
      fc.array(fc.nat({ max: 100 }), {
        minLength: 1,
        maxLength: 6,
      }),
      (seed, steps) => {
        const path = steps.join(":");
        expect(
          propertyConfig({
            env: {
              PROPERTY_TEST_SEED: String(seed),
              PROPERTY_TEST_PATH: path,
            },
          }),
        ).toMatchObject({ seed, path });
      },
    ),
  );
});

test("invalid replay configuration is refused", () => {
  for (const env of [
    { PROPERTY_TEST_SEED: "NaN" },
    { PROPERTY_TEST_SEED: "2147483648" },
    { PROPERTY_TEST_PATH: "1::2" },
    { PROPERTY_TEST_NUM_RUNS_FACTOR: "0" },
    { PROPERTY_TEST_NUM_RUNS_FACTOR: "1.5" },
  ]) {
    expect(() => propertyConfig({ env })).toThrow(
      PropertyTestConfigError,
    );
  }
});
