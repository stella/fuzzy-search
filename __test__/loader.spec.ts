import { describe, expect, test } from "bun:test";
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

const isLoaderResult = (value: unknown): value is LoaderResult => {
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

const loadBinding = (scenario: LoaderScenario) => {
  const script = `
    const Module = require("node:module");
    const scenario = JSON.parse(process.argv[1]);
    const originalLoad = Module._load;
    const bindings = {
      localWasi: { source: "local-wasi" },
      native: { source: "native" },
      packageWasi: { source: "package-wasi" },
    };
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
      const binding = require(${JSON.stringify(loaderPath)});
      process.stdout.write(JSON.stringify({ source: binding.source }));
    } catch (error) {
      process.stdout.write(JSON.stringify({
        causeEnumerable: Object.prototype.propertyIsEnumerable.call(error, "cause"),
        causePresent: "cause" in error,
        error: error.message,
      }));
    }
  `;
  const result = spawnSync(
    "node",
    ["-e", script, JSON.stringify(scenario)],
    {
      encoding: "utf8",
    },
  );

  expect(result.status).toBe(0);
  const parsed: unknown = JSON.parse(result.stdout);
  if (!isLoaderResult(parsed)) {
    throw new Error(`Unexpected loader result: ${result.stdout}`);
  }
  return parsed;
};

describe("generated loader WASI selection", () => {
  for (const forceWasi of [undefined, "false", "0", "1"]) {
    test(`${forceWasi ?? "unset"} keeps the native binding`, () => {
      expect(
        loadBinding({ forceWasi, native: true }),
      ).toEqual({
        source: "native",
      });
    });
  }

  test("true retains native when WASI is unavailable", () => {
    expect(
      loadBinding({ forceWasi: "true", native: true }),
    ).toEqual({
      source: "native",
    });
  });

  test("true selects the packaged WASI candidate last", () => {
    expect(
      loadBinding({
        forceWasi: "true",
        localWasi: true,
        native: true,
        packageWasi: true,
      }),
    ).toEqual({ source: "package-wasi" });
  });

  test("missing native falls back to local WASI", () => {
    expect(loadBinding({ localWasi: true })).toEqual({
      source: "local-wasi",
    });
  });

  test("error requires an available WASI binding", () => {
    expect(
      loadBinding({ forceWasi: "error", native: true }),
    ).toEqual({
      causeEnumerable: false,
      causePresent: true,
      error:
        "WASI binding not found and NAPI_RS_FORCE_WASI is set to error",
    });
  });

  test("loader failure preserves a non-enumerable cause", () => {
    const result = loadBinding({});

    expect(result.causePresent).toBe(true);
    expect(result.causeEnumerable).toBe(false);
    expect(result.error).toContain("Cannot find native binding");
  });
});
