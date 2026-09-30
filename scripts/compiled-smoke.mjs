import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const builtSource = readFileSync(
  join(root, "dist/index.mjs"),
  "utf8",
);
const directory = mkdtempSync(
  join(tmpdir(), "fuzzy-compiled-"),
);
try {
  // Stage the shipped native package layout. The checkout additionally has
  // build-only WASI fallbacks that are not published in the root package.
  const modules = join(directory, "node_modules");
  const packageDirectory = join(
    modules,
    "@stll/fuzzy-search",
  );
  mkdirSync(join(packageDirectory, "dist"), {
    recursive: true,
  });
  for (const file of [
    "package.json",
    "index.cjs",
    "dist/index.mjs",
  ]) {
    copyFileSync(
      join(root, file),
      join(packageDirectory, file),
    );
  }
  let nativePackages = 0;
  for (const platform of readdirSync(join(root, "npm"))) {
    const manifestPath = join(
      root,
      "npm",
      platform,
      "package.json",
    );
    const manifest = JSON.parse(
      readFileSync(manifestPath, "utf8"),
    );
    if (!manifest.main.endsWith(".node")) continue;
    const artifact = join(root, manifest.main);
    if (!existsSync(artifact)) continue;
    const nativeDirectory = join(modules, manifest.name);
    mkdirSync(nativeDirectory, { recursive: true });
    copyFileSync(
      manifestPath,
      join(nativeDirectory, "package.json"),
    );
    copyFileSync(
      artifact,
      join(nativeDirectory, manifest.main),
    );
    nativePackages += 1;
  }
  assert.ok(
    nativePackages > 0,
    "Build a native artifact before the compiled smoke",
  );

  const entry = join(directory, "probe.ts");
  const executable = join(
    directory,
    process.platform === "win32" ? "probe.exe" : "probe",
  );
  writeFileSync(
    entry,
    'import { distance } from "@stll/fuzzy-search";\nconsole.log(distance("Novak", "Nowak"));\n',
  );
  const compiled = spawnSync(
    process.execPath,
    ["build", "--compile", entry, "--outfile", executable],
    {
      cwd: directory,
      encoding: "utf8",
      timeout: 60_000,
    },
  );
  assert.ifError(compiled.error);
  assert.equal(compiled.status, 0, compiled.stderr);

  // Neither the package nor its binding may be available from disk at runtime.
  rmSync(modules, { recursive: true, force: true });
  rmSync(entry);
  const executed = spawnSync(executable, [], {
    cwd: directory,
    encoding: "utf8",
    timeout: 10_000,
  });
  assert.ifError(executed.error);
  assert.equal(executed.status, 0, executed.stderr);
  assert.equal(executed.stdout.trim(), "1");
  assert.match(
    builtSource,
    /import\s+[^;]+from\s*["']\.\.\/index\.cjs["']/,
  );
  assert.doesNotMatch(builtSource, /createRequire/);
} finally {
  rmSync(directory, { recursive: true, force: true });
}
