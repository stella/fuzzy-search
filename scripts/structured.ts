// Typed access to parsed JSON, YAML and TOML documents in repository tests.
// Each accessor narrows `unknown` and fails with the path it expected.

export type Mapping = Record<string, unknown>;

const isMapping = (value: unknown): value is Mapping =>
  typeof value === "object" && value !== null && !Array.isArray(value);

export const mapping = (value: unknown, label: string): Mapping => {
  if (!isMapping(value)) throw new TypeError(`${label} must be a mapping`);
  return value;
};

export const list = (value: unknown, label: string): unknown[] => {
  if (!Array.isArray(value)) throw new TypeError(`${label} must be a list`);
  return value;
};

export const text = (value: unknown, label: string): string => {
  if (typeof value !== "string") {
    throw new TypeError(`${label} must be a string`);
  }
  return value;
};

/** Follows `keys` through nested mappings; the last value is returned as is. */
export const at = (value: unknown, ...keys: string[]): unknown => {
  let current = value;
  let label = "document";
  for (const key of keys) {
    current = mapping(current, label)[key];
    label = `${label}.${key}`;
  }
  return current;
};

export const parseJson = (source: string): unknown => JSON.parse(source);
