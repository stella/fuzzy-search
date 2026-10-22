import { spawnSync } from "node:child_process";
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

await import("./fix-napi-loader.mjs");
