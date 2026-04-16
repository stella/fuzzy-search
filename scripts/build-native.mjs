import { spawnSync } from "node:child_process";
import { access, rename, rm } from "node:fs/promises";
import { constants } from "node:fs";
import { fileURLToPath } from "node:url";

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

const source = new URL("../index.js", import.meta.url);
const target = new URL("../index.cjs", import.meta.url);

try {
  await access(source, constants.F_OK);
} catch {
  process.exit(0);
}

await rm(target, { force: true });
await rename(source, target);
