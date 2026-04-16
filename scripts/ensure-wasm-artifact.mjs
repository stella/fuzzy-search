import { access } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { constants } from "node:fs";
import { fileURLToPath } from "node:url";

const wasmPath = fileURLToPath(
  new URL("../fuzzy-search.wasm32-wasi.wasm", import.meta.url),
);
const browserPath = fileURLToPath(
  new URL("../fuzzy-search.wasi-browser.js", import.meta.url),
);
const napiPath = fileURLToPath(
  new URL(
    `../node_modules/.bin/${process.platform === "win32" ? "napi.cmd" : "napi"}`,
    import.meta.url,
  ),
);

try {
  await access(wasmPath, constants.F_OK);
  await access(browserPath, constants.F_OK);
} catch (error) {
  if (error?.code !== "ENOENT") {
    throw error;
  }

  const result = spawnSync(
    napiPath,
    ["build", "--platform", "--target", "wasm32-wasip1-threads", "--release"],
    {
      shell: process.platform === "win32",
      stdio: "inherit",
    },
  );

  if (result.error) {
    throw result.error;
  }

  if ((result.status ?? 1) !== 0) {
    process.exit(result.status ?? 1);
  }

  await import("./fix-napi-loader.mjs");
}
