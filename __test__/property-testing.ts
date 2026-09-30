import fc from "fast-check";

import seeds from "./property-seeds.json";

const FIXED_SEED = -530956778;
const DEFAULT_RUNS = 200;

export class PropertyTestConfigError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PropertyTestConfigError";
  }
}

const integerEnv = (
  raw: string | undefined,
  fallback: number,
): number => {
  if (raw === undefined) return fallback;
  if (
    !/^-?\d+$/u.test(raw) ||
    !Number.isSafeInteger(Number(raw))
  ) {
    throw new PropertyTestConfigError(
      "Property test configuration needs an integer.",
    );
  }
  return Number(raw);
};

type PropertyTestEnvironment = Readonly<
  Record<string, string | undefined> & {
    PROPERTY_TEST_NUM_RUNS_FACTOR?: string | undefined;
    PROPERTY_TEST_SEED?: string | undefined;
    PROPERTY_TEST_PATH?: string | undefined;
  }
>;

type PropertyConfigOptions<T> = {
  params?: Omit<fc.Parameters<T>, "seed" | "path">;
  env?: PropertyTestEnvironment;
};

const runFactor = (
  env: PropertyTestEnvironment,
): number => {
  const factor = integerEnv(
    env.PROPERTY_TEST_NUM_RUNS_FACTOR,
    1,
  );
  if (factor < 1) {
    throw new PropertyTestConfigError(
      "Property test run factor must be positive.",
    );
  }
  return factor;
};

export const propertyTestTimeout = (
  baseMs: number,
): number => {
  const timeout = baseMs * runFactor(process.env);
  if (!Number.isSafeInteger(timeout) || timeout < 1) {
    throw new PropertyTestConfigError(
      "Property test timeout must be a positive integer.",
    );
  }
  return timeout;
};

export const propertyConfig = <T>({
  params = {},
  env = process.env,
}: PropertyConfigOptions<T> = {}): fc.Parameters<T> => {
  const factor = runFactor(env);
  const seed = integerEnv(
    env.PROPERTY_TEST_SEED,
    FIXED_SEED,
  );
  if (seed < -2_147_483_648 || seed > 2_147_483_647) {
    throw new PropertyTestConfigError(
      "Property test seed must fit a signed 32-bit integer.",
    );
  }
  const path = env.PROPERTY_TEST_PATH;
  if (path !== undefined && !/^\d+(?::\d+)*$/u.test(path)) {
    throw new PropertyTestConfigError(
      "Property test replay path needs colon-separated integers.",
    );
  }
  const numRuns = (params.numRuns ?? DEFAULT_RUNS) * factor;
  if (!Number.isSafeInteger(numRuns) || numRuns < 1) {
    throw new PropertyTestConfigError(
      "Property test run budget must be a positive integer.",
    );
  }
  return {
    ...params,
    numRuns,
    seed,
    ...(path === undefined ? {} : { path }),
  };
};

/** Committed replays always precede the deterministic generated pass. */
export const assertProperty = <T>(
  id: string,
  property: fc.IProperty<T>,
  params?: Omit<fc.Parameters<T>, "seed" | "path">,
): void => {
  const config = propertyConfig(
    params === undefined ? {} : { params },
  );
  const pinned =
    Object.entries(seeds).find(
      ([key]) => key === id,
    )?.[1] ?? [];
  for (const { seed, path } of pinned) {
    fc.assert(property, {
      seed,
      path,
      numRuns: 1,
      endOnFailure: true,
    });
  }
  fc.assert(property, config);
};
