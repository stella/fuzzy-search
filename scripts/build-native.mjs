import { spawnSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import { normalizeGeneratedLoader } from "./generated-loader-normalization.mjs";

const cliPath = new URL(
  "../node_modules/@napi-rs/cli/dist/cli.js",
  import.meta.url,
);

const result = spawnSync(
  process.execPath,
  [fileURLToPath(cliPath), "build", ...process.argv.slice(2)],
  {
    stdio: "inherit",
  },
);

if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

async function normalizeGeneratedFile(fileName, options) {
  const fileUrl = new URL(`../${fileName}`, import.meta.url);
  try {
    const content = await readFile(fileUrl, "utf8");
    await writeFile(fileUrl, normalizeGeneratedLoader(content, options));
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
}

await normalizeGeneratedFile("fuzzy-search.wasi-browser.js", {
  keepGeneratedTypecheckOptOut: true,
});
await normalizeGeneratedFile("fuzzy-search.wasi.cjs", {
  keepGeneratedTypecheckOptOut: true,
});
await normalizeGeneratedFile("wasi-worker-browser.mjs");
await normalizeGeneratedFile("wasi-worker.mjs");

await import("./fix-napi-loader.mjs");
