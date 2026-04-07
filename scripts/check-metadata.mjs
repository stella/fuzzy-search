import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = path.dirname(
  fileURLToPath(
    new URL("../package.json", import.meta.url),
  ),
);
const rootPkg = JSON.parse(
  readFileSync(path.join(rootDir, "package.json"), "utf8"),
);
const version = rootPkg.version;

const cargoToml = readFileSync(
  path.join(rootDir, "Cargo.toml"),
  "utf8",
);
const cargoVersion = cargoToml.match(
  /^version = "([^"]+)"$/m,
)?.[1];
assert.equal(
  cargoVersion,
  version,
  "Cargo.toml version must match package.json",
);

const wasmPkg = JSON.parse(
  readFileSync(
    path.join(rootDir, "wasm/package.json"),
    "utf8",
  ),
);
assert.equal(
  wasmPkg.version,
  version,
  "wasm/package.json version must match package.json",
);

for (const [name, depVersion] of Object.entries(
  rootPkg.optionalDependencies ?? {},
)) {
  assert.equal(
    depVersion,
    version,
    `optional dependency ${name} must match package.json version`,
  );
}

for (const dirent of readdirSync(
  path.join(rootDir, "npm"),
  { withFileTypes: true },
)) {
  if (!dirent.isDirectory()) {
    continue;
  }

  const dir = dirent.name;
  const pkg = JSON.parse(
    readFileSync(
      path.join(rootDir, "npm", dir, "package.json"),
      "utf8",
    ),
  );
  assert.equal(
    pkg.version,
    version,
    `npm/${dir}/package.json version must match package.json`,
  );
}

const loader = readFileSync(
  path.join(rootDir, "index.js"),
  "utf8",
);
assert.match(
  loader,
  /import \{ createRequire \} from 'node:module'/,
  "index.js must be generated in ESM mode",
);

const expectedVersions = [
  ...loader.matchAll(/expected ([0-9]+\.[0-9]+\.[0-9]+)/g),
].map((match) => match[1]);
assert.ok(
  expectedVersions.length > 0,
  "index.js must contain version checks",
);
for (const expectedVersion of expectedVersions) {
  assert.equal(
    expectedVersion,
    version,
    "index.js version checks must match package.json",
  );
}

process.stdout.write(
  `Metadata is consistent at ${version}\n`,
);
