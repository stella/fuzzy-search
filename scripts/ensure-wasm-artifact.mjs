import { spawnSync } from "node:child_process";
import { constants } from "node:fs";
import { access } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const wasmPath = fileURLToPath(
  new URL("../fuzzy-search.wasm32-wasi.wasm", import.meta.url),
);
const browserPath = fileURLToPath(
  new URL("../fuzzy-search.wasi-browser.js", import.meta.url),
);
const root = fileURLToPath(new URL("../", import.meta.url));

try {
  await access(wasmPath, constants.F_OK);
  await access(browserPath, constants.F_OK);
} catch (error) {
  if (error?.code !== "ENOENT") {
    throw error;
  }

  // build:wasm runs the generated loaders through the same normalization and
  // loader repair as every other build.
  const result = spawnSync("bun", ["run", "build:wasm"], {
    cwd: root,
    shell: process.platform === "win32",
    stdio: "inherit",
  });

  if (result.error) {
    throw result.error;
  }

  if ((result.status ?? 1) !== 0) {
    process.exit(result.status ?? 1);
  }
}
