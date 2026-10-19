import { constants } from "node:fs";
import {
  access,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";

import { transformNapiLoader } from "./napi-loader-transform.mjs";

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
