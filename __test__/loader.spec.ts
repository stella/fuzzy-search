import {
  beforeAll,
  describe,
  expect,
  test,
} from "bun:test";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const loaderPath = fileURLToPath(
  new URL("../index.cjs", import.meta.url),
);

type LoaderScenario = {
  forceWasi?: string;
  localWasi?: boolean;
  native?: boolean;
  packageWasi?: boolean;
};

type LoaderResult = {
  causeEnumerable?: boolean;
  causePresent?: boolean;
  error?: string;
  source?: string;
};

const isOptionalBoolean = (value: unknown) =>
  value === undefined || typeof value === "boolean";

const isOptionalString = (value: unknown) =>
  value === undefined || typeof value === "string";

const isLoaderResult = (
  value: unknown,
): value is LoaderResult => {
  if (typeof value !== "object" || value === null) {
    return false;
  }

  const result = Object.fromEntries(Object.entries(value));
  return (
    isOptionalBoolean(result.causeEnumerable) &&
    isOptionalBoolean(result.causePresent) &&
    isOptionalString(result.error) &&
    isOptionalString(result.source)
  );
};

const scenarios = {
  unset: { native: true },
  false: { forceWasi: "false", native: true },
  "0": { forceWasi: "0", native: true },
  "1": { forceWasi: "1", native: true },
  unavailableWasi: { forceWasi: "true", native: true },
  packagedWasi: {
    forceWasi: "true",
    localWasi: true,
    native: true,
    packageWasi: true,
  },
  localWasi: { localWasi: true },
  requiredWasi: { forceWasi: "error", native: true },
  unavailable: {},
} satisfies Record<string, LoaderScenario>;

type LoaderResults = Record<
  keyof typeof scenarios,
  LoaderResult
>;

const isLoaderResults = (
  value: unknown,
): value is LoaderResults => {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const results = Object.fromEntries(Object.entries(value));
  const keys = Object.keys(scenarios);
  return (
    Object.keys(results).length === keys.length &&
    keys.every(
      (key) =>
        Object.hasOwn(results, key) &&
        isLoaderResult(results[key]),
    )
  );
};

const loadBindings = () => {
  const script = `
    const Module = require("node:module");
    const scenarios = JSON.parse(process.argv[1]);
    const loaderPath = require.resolve(process.argv[2]);
    const originalLoad = Module._load;
    const originalForceWasi = process.env.NAPI_RS_FORCE_WASI;
    const originalExcludeNetwork = process.report &&
      Object.getOwnPropertyDescriptor(process.report, "excludeNetwork");
    const loadBinding = (scenario) => {
      const bindings = {
        localWasi: { source: "local-wasi" },
        native: { source: "native" },
        packageWasi: { source: "package-wasi" },
      };
      delete require.cache[loaderPath];
      Module._load = (request, parent, isMain) => {
        if (request.endsWith(".node")) {
          if (scenario.native) return bindings.native;
          throw Object.assign(new Error("native unavailable"), { code: "MODULE_NOT_FOUND" });
        }
        if (request === "./fuzzy-search.wasi.cjs") {
          if (scenario.localWasi) return bindings.localWasi;
          throw Object.assign(new Error("local WASI unavailable"), { code: "MODULE_NOT_FOUND" });
        }
        if (request === "@stll/fuzzy-search-wasm32-wasi") {
          if (scenario.packageWasi) return bindings.packageWasi;
          throw Object.assign(new Error("package WASI unavailable"), { code: "MODULE_NOT_FOUND" });
        }
        if (request.startsWith("@stll/fuzzy-search-")) {
          throw Object.assign(new Error("native package unavailable"), { code: "MODULE_NOT_FOUND" });
        }
        return originalLoad(request, parent, isMain);
      };
      if (scenario.forceWasi === undefined) {
        delete process.env.NAPI_RS_FORCE_WASI;
      } else {
        process.env.NAPI_RS_FORCE_WASI = scenario.forceWasi;
      }
      try {
        const binding = require(loaderPath);
        return { source: binding.source };
      } catch (error) {
        return {
          causeEnumerable: Object.prototype.propertyIsEnumerable.call(error, "cause"),
          causePresent: "cause" in error,
          error: error.message,
        };
      } finally {
        // Each scenario owns the loader cache and the globals it can change.
        delete require.cache[loaderPath];
        Module._load = originalLoad;
        if (originalForceWasi === undefined) {
          delete process.env.NAPI_RS_FORCE_WASI;
        } else {
          process.env.NAPI_RS_FORCE_WASI = originalForceWasi;
        }
        if (process.report) {
          if (originalExcludeNetwork) {
            Object.defineProperty(process.report, "excludeNetwork", originalExcludeNetwork);
          } else {
            delete process.report.excludeNetwork;
          }
        }
      }
    };
    const results = Object.fromEntries(
      Object.entries(scenarios).map(([key, scenario]) => [key, loadBinding(scenario)]),
    );
    process.stdout.write(JSON.stringify(results));
  `;
  const result = spawnSync(
    "node",
    ["-e", script, JSON.stringify(scenarios), loaderPath],
    {
      encoding: "utf8",
    },
  );

  expect(result.status).toBe(0);
  const parsed: unknown = JSON.parse(result.stdout);
  if (!isLoaderResults(parsed)) {
    throw new Error(
      `Unexpected loader results: ${result.stdout}`,
    );
  }
  return parsed;
};

describe("generated loader WASI selection", () => {
  let results: LoaderResults;
  beforeAll(() => {
    results = loadBindings();
  });

  for (const forceWasi of [
    "unset",
    "false",
    "0",
    "1",
  ] as const) {
    test(`${forceWasi} keeps the native binding`, () => {
      expect(results[forceWasi]).toEqual({
        source: "native",
      });
    });
  }

  test("true retains native when WASI is unavailable", () => {
    expect(results.unavailableWasi).toEqual({
      source: "native",
    });
  });

  test("true selects the packaged WASI candidate last", () => {
    expect(results.packagedWasi).toEqual({
      source: "package-wasi",
    });
  });

  test("missing native falls back to local WASI", () => {
    expect(results.localWasi).toEqual({
      source: "local-wasi",
    });
  });

  test("error requires an available WASI binding", () => {
    expect(results.requiredWasi).toEqual({
      causeEnumerable: false,
      causePresent: true,
      error:
        "WASI binding not found and NAPI_RS_FORCE_WASI is set to error",
    });
  });

  test("loader failure preserves a non-enumerable cause", () => {
    const result = results.unavailable;

    expect(result.causePresent).toBe(true);
    expect(result.causeEnumerable).toBe(false);
    expect(result.error).toContain(
      "Cannot find native binding",
    );
  });
});
