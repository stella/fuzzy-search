import { constants } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  access,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import {
  stripDirectiveComments,
  transformNapiLoader,
} from "./napi-loader-transform.mjs";

const source = new URL("../index.js", import.meta.url);
const target = new URL("../index.cjs", import.meta.url);

let hasGeneratedLoader = true;
try {
  await access(source, constants.F_OK);
} catch {
  hasGeneratedLoader = false;
}

if (hasGeneratedLoader) {
  await rm(target, { force: true });
  await rename(source, target);
} else {
  await access(target, constants.F_OK);
}

const loader = await readFile(target, "utf8");
await writeFile(target, transformNapiLoader(loader));

const generatedCompanions = [
  new URL("../index.d.ts", import.meta.url),
  new URL("../fuzzy-search.wasi.cjs", import.meta.url),
  new URL("../wasi-worker-browser.mjs", import.meta.url),
  new URL("../wasi-worker.mjs", import.meta.url),
];
for (const generatedFile of generatedCompanions) {
  try {
    const generatedSource = await readFile(generatedFile, "utf8");
    await writeFile(
      generatedFile,
      stripDirectiveComments(generatedSource),
    );
  } catch (error) {
    if (error.code !== "ENOENT") {
      throw error;
    }
  }
}

execFileSync(
  fileURLToPath(
    new URL("../node_modules/oxfmt/bin/oxfmt", import.meta.url),
  ),
  ["wasi-worker-browser.mjs", "wasi-worker.mjs"],
  { cwd: fileURLToPath(new URL("..", import.meta.url)) },
);
