#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const ROOT = new URL("../", import.meta.url);

function repoPath(...segments) {
  return path.join(new URL(ROOT).pathname, ...segments);
}

function readText(filePath) {
  return fs.readFileSync(filePath, "utf8");
}

function writeText(filePath, content) {
  fs.writeFileSync(filePath, content);
}

function readJson(filePath) {
  return JSON.parse(readText(filePath));
}

function writeJson(filePath, value) {
  writeText(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

function fileExists(filePath) {
  return fs.existsSync(filePath);
}

function replaceRequired(content, matcher, replacement, filePath) {
  if (!matcher.test(content)) {
    throw new Error(`Expected to match ${matcher} in ${filePath}`);
  }
  return content.replace(matcher, replacement);
}

function packageMeta() {
  const root = readJson(repoPath("package.json"));
  const cargoTomlPath = repoPath("Cargo.toml");
  const cargoToml = readText(cargoTomlPath);
  const cargoNameMatch = cargoToml.match(/^name = "([^"]+)"$/m);
  if (!cargoNameMatch) {
    throw new Error(`Missing Cargo package name in ${cargoTomlPath}`);
  }

  const [scope, baseName] = root.name.split("/");
  if (!scope || !baseName) {
    throw new Error(`Expected scoped package name, got ${root.name}`);
  }

  return {
    root,
    cargoName: cargoNameMatch[1],
    optionalPrefix: `${scope}/${baseName}-`,
    packageJsonPath: repoPath("package.json"),
    cargoTomlPath,
    cargoLockPath: repoPath("Cargo.lock"),
    bunLockPath: repoPath("bun.lock"),
    indexCjsPath: repoPath("index.cjs"),
    provenanceSbomPath: repoPath("provenance", "sbom.cdx.json"),
    wasmManifestPath: repoPath("wasm", "package.json"),
  };
}

function mismatches(expectedVersion) {
  const meta = packageMeta();
  const results = [];

  if (meta.root.version !== expectedVersion) {
    results.push(`${meta.packageJsonPath}: version=${meta.root.version}`);
  }

  for (const [name, version] of Object.entries(meta.root.optionalDependencies ?? {})) {
    if (name.startsWith(meta.optionalPrefix) && version !== expectedVersion) {
      results.push(`${meta.packageJsonPath}: optionalDependencies.${name}=${version}`);
    }
  }

  const wasm = readJson(meta.wasmManifestPath);
  if (wasm.version !== expectedVersion) {
    results.push(`${meta.wasmManifestPath}: version=${wasm.version}`);
  }

  for (const entry of fs.readdirSync(repoPath("npm"), { withFileTypes: true })) {
    if (!entry.isDirectory()) {
      continue;
    }
    const manifestPath = repoPath("npm", entry.name, "package.json");
    const manifest = readJson(manifestPath);
    if (manifest.version !== expectedVersion) {
      results.push(`${manifestPath}: version=${manifest.version}`);
    }
  }

  const cargoToml = readText(meta.cargoTomlPath);
  const cargoTomlMatch = cargoToml.match(/^version = "([^"]+)"$/m);
  if (!cargoTomlMatch || cargoTomlMatch[1] !== expectedVersion) {
    results.push(`${meta.cargoTomlPath}: version=${cargoTomlMatch?.[1] ?? "<missing>"}`);
  }

  const cargoLock = readText(meta.cargoLockPath);
  const cargoLockMatch = cargoLock.match(
    new RegExp(`\\[\\[package\\]\\]\\nname = "${meta.cargoName}"\\nversion = "([^"]+)"`),
  );
  if (!cargoLockMatch || cargoLockMatch[1] !== expectedVersion) {
    results.push(`${meta.cargoLockPath}: version=${cargoLockMatch?.[1] ?? "<missing>"}`);
  }

  const bunLock = readText(meta.bunLockPath);
  for (const pkg of Object.keys(meta.root.optionalDependencies ?? {}).filter((name) =>
    name.startsWith(meta.optionalPrefix),
  )) {
    const escaped = pkg.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const match = bunLock.match(new RegExp(`"${escaped}": "([^"]+)"`));
    if (!match || match[1] !== expectedVersion) {
      results.push(`${meta.bunLockPath}: ${pkg}=${match?.[1] ?? "<missing>"}`);
    }
  }

  if (fileExists(meta.indexCjsPath)) {
    const indexCjs = readText(meta.indexCjsPath);
    if (!indexCjs.includes(`expected ${expectedVersion} but got`)) {
      results.push(`${meta.indexCjsPath}: expected version string ${expectedVersion}`);
    }
  }

  if (fileExists(meta.provenanceSbomPath)) {
    const provenanceSbom = readText(meta.provenanceSbomPath);
    if (!provenanceSbom.includes(`pkg:npm/${meta.root.name}@${expectedVersion}`)) {
      results.push(`${meta.provenanceSbomPath}: npm purl not updated to ${expectedVersion}`);
    }
    if (!provenanceSbom.includes(`pkg:cargo/${meta.cargoName}@${expectedVersion}`)) {
      results.push(`${meta.provenanceSbomPath}: cargo purl not updated to ${expectedVersion}`);
    }
  }

  return results;
}

function syncVersion(nextVersion) {
  const meta = packageMeta();

  meta.root.version = nextVersion;
  for (const name of Object.keys(meta.root.optionalDependencies ?? {})) {
    if (name.startsWith(meta.optionalPrefix)) {
      meta.root.optionalDependencies[name] = nextVersion;
    }
  }
  writeJson(meta.packageJsonPath, meta.root);

  const wasmManifest = readJson(meta.wasmManifestPath);
  wasmManifest.version = nextVersion;
  writeJson(meta.wasmManifestPath, wasmManifest);

  for (const entry of fs.readdirSync(repoPath("npm"), { withFileTypes: true })) {
    if (!entry.isDirectory()) {
      continue;
    }
    const manifestPath = repoPath("npm", entry.name, "package.json");
    const manifest = readJson(manifestPath);
    manifest.version = nextVersion;
    writeJson(manifestPath, manifest);
  }

  let cargoToml = readText(meta.cargoTomlPath);
  cargoToml = replaceRequired(
    cargoToml,
    /^version = "([^"]+)"$/m,
    `version = "${nextVersion}"`,
    meta.cargoTomlPath,
  );
  writeText(meta.cargoTomlPath, cargoToml);

  let cargoLock = readText(meta.cargoLockPath);
  cargoLock = replaceRequired(
    cargoLock,
    new RegExp(`(\\[\\[package\\]\\]\\nname = "${meta.cargoName}"\\nversion = ")[^"]+(")`),
    `$1${nextVersion}$2`,
    meta.cargoLockPath,
  );
  writeText(meta.cargoLockPath, cargoLock);

  let bunLock = readText(meta.bunLockPath);
  for (const pkg of Object.keys(meta.root.optionalDependencies ?? {}).filter((name) =>
    name.startsWith(meta.optionalPrefix),
  )) {
    const escaped = pkg.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    bunLock = replaceRequired(
      bunLock,
      new RegExp(`("${escaped}": ")([^"]+)(")`),
      `$1${nextVersion}$3`,
      meta.bunLockPath,
    );
  }
  writeText(meta.bunLockPath, bunLock);

  if (fileExists(meta.indexCjsPath)) {
    let indexCjs = readText(meta.indexCjsPath);
    indexCjs = replaceRequired(
      indexCjs,
      /expected [^ ]+ but got/g,
      `expected ${nextVersion} but got`,
      meta.indexCjsPath,
    );
    writeText(meta.indexCjsPath, indexCjs);
  }

  if (fileExists(meta.provenanceSbomPath)) {
    let provenanceSbom = readText(meta.provenanceSbomPath);
    provenanceSbom = replaceRequired(
      provenanceSbom,
      new RegExp(
        `pkg:npm/${meta.root.name.replace("/", "\\/")}@[^"\\s<]+`,
        "g",
      ),
      `pkg:npm/${meta.root.name}@${nextVersion}`,
      meta.provenanceSbomPath,
    );
    provenanceSbom = replaceRequired(
      provenanceSbom,
      new RegExp(`pkg:cargo/${meta.cargoName}@[^"\\s<]+`, "g"),
      `pkg:cargo/${meta.cargoName}@${nextVersion}`,
      meta.provenanceSbomPath,
    );
    writeText(meta.provenanceSbomPath, provenanceSbom);
  }
}

function parseArgs() {
  const [command, ...rest] = process.argv.slice(2);
  const args = new Map();
  for (let i = 0; i < rest.length; i += 1) {
    const token = rest[i];
    if (token === "--version") {
      args.set("version", rest[i + 1]);
      i += 1;
      continue;
    }
    if (token === "--tag") {
      args.set("tag", rest[i + 1]);
      i += 1;
    }
  }
  return { command, args };
}

function main() {
  const { command, args } = parseArgs();
  const rootVersion = packageMeta().root.version;

  if (command === "sync") {
    syncVersion(args.get("version") ?? rootVersion);
    return;
  }

  if (command === "check") {
    const expectedVersion = args.get("tag")
      ? args.get("tag").replace(/^v/, "")
      : rootVersion;
    const drift = mismatches(expectedVersion);
    if (drift.length > 0) {
      console.error("Version drift detected:");
      for (const mismatch of drift) {
        console.error(`- ${mismatch}`);
      }
      process.exit(1);
    }
    return;
  }

  console.error(
    "Usage: node scripts/version-sync.mjs <sync|check> [--version <semver>] [--tag <git-tag>]",
  );
  process.exit(1);
}

main();
