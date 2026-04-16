import { access, rename, rm } from "node:fs/promises";
import { constants } from "node:fs";

const source = new URL("../index.js", import.meta.url);
const target = new URL("../index.cjs", import.meta.url);

try {
  await access(source, constants.F_OK);
} catch {
  process.exit(0);
}

await rm(target, { force: true });
await rename(source, target);
