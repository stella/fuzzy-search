#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../", import.meta.url));

function repoPath(...segments) {
  return path.join(ROOT, ...segments);
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
  writeText(
    filePath,
    `${JSON.stringify(value, null, 2)}\n`,
  );
}

function readVersion() {
  return readText(repoPath("VERSION")).trim();
}

function fileExists(filePath) {
  return fs.existsSync(filePath);
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function replaceRequired(
  content,
  matcher,
  replacement,
  filePath,
) {
  if (!matcher.test(content)) {
    throw new Error(
      `Expected to match ${matcher} in ${filePath}`,
    );
  }
  return content.replace(matcher, replacement);
}

function replaceIfPresent(content, matcher, replacement) {
  return matcher.test(content)
    ? content.replace(matcher, replacement)
    : content;
}

function encodedScopedPackageName(name) {
  if (!name.startsWith("@")) {
    return name;
  }

  const [scope, packageName] = name.slice(1).split("/");
  return `%40${scope}/${packageName}`;
}

function npmPurlCandidates(name, version) {
  return [
    `pkg:npm/${name}@${version}`,
    `pkg:npm/${encodedScopedPackageName(name)}@${version}`,
  ];
}

function npmPurlPrefixes(name) {
  return [
    `pkg:npm/${name}@`,
    `pkg:npm/${encodedScopedPackageName(name)}@`,
  ];
}

function readBunLockVersion(bunLock, packageName) {
  const escaped = escapeRegex(packageName);
  const patterns = [
    new RegExp(`"${escaped}": "([^"]+)"`),
    new RegExp(`"${escaped}": \\["${escaped}@([^"]+)"`),
  ];

  for (const pattern of patterns) {
    const match = bunLock.match(pattern);
    if (match) {
      return match[1];
    }
  }

  return null;
}

function collectSbomVersionDrift(
  value,
  expectedPurls,
  expectedVersion,
  results,
) {
  if (Array.isArray(value)) {
    for (const item of value) {
      collectSbomVersionDrift(
        item,
        expectedPurls,
        expectedVersion,
        results,
      );
    }
    return;
  }

  if (typeof value !== "object" || value === null) {
    return;
  }

  if (
    typeof value.purl === "string" &&
    expectedPurls.has(value.purl) &&
    value.version !== expectedVersion
  ) {
    results.push(
      `${value.purl}: version=${value.version ?? "<missing>"}`,
    );
  }

  for (const child of Object.values(value)) {
    collectSbomVersionDrift(
      child,
      expectedPurls,
      expectedVersion,
      results,
    );
  }
}

function replaceSbomVersionForPurl(
  provenanceSbom,
  purl,
  nextVersion,
) {
  return provenanceSbom.replace(
    new RegExp(
      `("purl": "${escapeRegex(purl)}",\\n\\s+"type": "[^"]+",\\n\\s+"version": ")[^"]+(")`,
      "g",
    ),
    `$1${nextVersion}$2`,
  );
}

function packageMeta() {
  const root = readJson(repoPath("package.json"));
  const cargoTomlPath = repoPath("Cargo.toml");
  const cargoToml = readText(cargoTomlPath);
  const cargoNameMatch = cargoToml.match(
    /^name = "([^"]+)"$/m,
  );
  if (!cargoNameMatch) {
    throw new Error(
      `Missing Cargo package name in ${cargoTomlPath}`,
    );
  }

  const [scope, baseName] = root.name.split("/");
  if (!scope || !baseName) {
    throw new Error(
      `Expected scoped package name, got ${root.name}`,
    );
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
    provenanceSbomPath: repoPath(
      "provenance",
      "sbom.cdx.json",
    ),
    wasmManifestPath: repoPath("wasm", "package.json"),
  };
}

function workspaceCargoTomlPaths() {
  const paths = [repoPath("Cargo.toml")];
  const cratesPath = repoPath("crates");
  if (!fileExists(cratesPath)) {
    return paths;
  }

  for (const entry of fs.readdirSync(cratesPath, {
    withFileTypes: true,
  })) {
    if (!entry.isDirectory()) {
      continue;
    }
    const cargoTomlPath = repoPath(
      "crates",
      entry.name,
      "Cargo.toml",
    );
    if (fileExists(cargoTomlPath)) {
      paths.push(cargoTomlPath);
    }
  }

  return paths;
}

function cargoPackageNames(cargoTomlPaths) {
  return cargoTomlPaths.map((cargoTomlPath) => {
    const cargoToml = readText(cargoTomlPath);
    const cargoNameMatch = cargoToml.match(
      /^name = "([^"]+)"$/m,
    );
    if (!cargoNameMatch) {
      throw new Error(
        `Missing Cargo package name in ${cargoTomlPath}`,
      );
    }
    return cargoNameMatch[1];
  });
}

function platformPackageManifests() {
  return fs
    .readdirSync(repoPath("npm"), {
      withFileTypes: true,
    })
    .filter((entry) => entry.isDirectory())
    .map((entry) => {
      const manifestPath = repoPath(
        "npm",
        entry.name,
        "package.json",
      );
      return {
        manifestPath,
        manifest: readJson(manifestPath),
      };
    })
    .sort((left, right) =>
      left.manifest.name.localeCompare(right.manifest.name),
    );
}

function mismatches(expectedVersion) {
  const meta = packageMeta();
  const platforms = platformPackageManifests();
  const results = [];
  const versionFileVersion = readVersion();

  if (versionFileVersion !== expectedVersion) {
    results.push(
      `${repoPath("VERSION")}: version=${versionFileVersion}`,
    );
  }
  if (meta.root.version !== expectedVersion) {
    results.push(
      `${meta.packageJsonPath}: version=${meta.root.version}`,
    );
  }

  const platformPackageNames = new Set(
    platforms.map(({ manifest }) => manifest.name),
  );
  const sourceOptionalPackages = Object.keys(
    meta.root.optionalDependencies ?? {},
  ).filter(
    (name) =>
      platformPackageNames.has(name) ||
      name.startsWith(meta.optionalPrefix),
  );
  for (const packageName of sourceOptionalPackages) {
    results.push(
      `${meta.packageJsonPath}: optionalDependencies.${packageName} should be release-generated`,
    );
  }

  const wasm = readJson(meta.wasmManifestPath);
  if (wasm.version !== expectedVersion) {
    results.push(
      `${meta.wasmManifestPath}: version=${wasm.version}`,
    );
  }

  for (const { manifestPath, manifest } of platforms) {
    if (manifest.version !== expectedVersion) {
      results.push(
        `${manifestPath}: version=${manifest.version}`,
      );
    }
    if (!manifest.name.startsWith(meta.optionalPrefix)) {
      results.push(
        `${manifestPath}: unexpected package name ${manifest.name}`,
      );
    }
  }

  const cargoTomlPaths = workspaceCargoTomlPaths();
  const cargoNames = cargoPackageNames(cargoTomlPaths);
  for (const cargoTomlPath of cargoTomlPaths) {
    const cargoToml = readText(cargoTomlPath);
    const cargoVersionMatch = cargoToml.match(
      /^version = "([^"]+)"$/m,
    );
    if (
      !cargoVersionMatch ||
      cargoVersionMatch[1] !== expectedVersion
    ) {
      results.push(
        `${cargoTomlPath}: version=${cargoVersionMatch?.[1] ?? "<missing>"}`,
      );
    }
    for (const cargoName of cargoNames) {
      const dependencyVersion = cargoToml.match(
        new RegExp(
          `^${escapeRegex(cargoName)} = \\{[^\\n]*version = "([^"]+)"`,
          "m",
        ),
      );
      if (
        dependencyVersion &&
        dependencyVersion[1] !== expectedVersion
      ) {
        results.push(
          `${cargoTomlPath}: dependencies.${cargoName}.version=${dependencyVersion[1]}`,
        );
      }
    }
  }

  const cargoLock = readText(meta.cargoLockPath);
  for (const cargoName of cargoNames) {
    const cargoLockMatch = cargoLock.match(
      new RegExp(
        `\\[\\[package\\]\\]\\nname = "${escapeRegex(cargoName)}"\\nversion = "([^"]+)"`,
      ),
    );
    if (
      !cargoLockMatch ||
      cargoLockMatch[1] !== expectedVersion
    ) {
      results.push(
        `${meta.cargoLockPath}: ${cargoName}.version=${cargoLockMatch?.[1] ?? "<missing>"}`,
      );
    }
  }

  const bunLock = readText(meta.bunLockPath);
  for (const packageName of platformPackageNames) {
    const lockfileVersion = readBunLockVersion(
      bunLock,
      packageName,
    );
    if (lockfileVersion !== null) {
      results.push(
        `${meta.bunLockPath}: ${packageName}=${lockfileVersion}; expected release-generated`,
      );
    }
  }

  if (fileExists(meta.indexCjsPath)) {
    const indexCjs = readText(meta.indexCjsPath);
    if (
      !indexCjs.includes(
        `bindingPackageVersion !== '${expectedVersion}'`,
      )
    ) {
      results.push(
        `${meta.indexCjsPath}: native binding guard not updated to ${expectedVersion}`,
      );
    }
    if (
      !indexCjs.includes(
        `expected ${expectedVersion} but got`,
      )
    ) {
      results.push(
        `${meta.indexCjsPath}: expected version string ${expectedVersion}`,
      );
    }
  }

  if (fileExists(meta.provenanceSbomPath)) {
    const provenanceSbom = readText(
      meta.provenanceSbomPath,
    );
    const expectedPurls = new Set([
      ...npmPurlCandidates(meta.root.name, expectedVersion),
      ...cargoNames.map(
        (cargoName) => `pkg:cargo/${cargoName}@${expectedVersion}`,
      ),
    ]);
    const hasRootNpmComponent = npmPurlPrefixes(
      meta.root.name,
    ).some((prefix) => provenanceSbom.includes(prefix));
    if (
      hasRootNpmComponent &&
      !npmPurlCandidates(
        meta.root.name,
        expectedVersion,
      ).some((candidate) =>
        provenanceSbom.includes(candidate),
      )
    ) {
      results.push(
        `${meta.provenanceSbomPath}: npm purl not updated to ${expectedVersion}`,
      );
    }
    for (const cargoName of cargoNames) {
      if (
        !provenanceSbom.includes(
          `pkg:cargo/${cargoName}@${expectedVersion}`,
        )
      ) {
        results.push(
          `${meta.provenanceSbomPath}: ${cargoName} cargo purl not updated to ${expectedVersion}`,
        );
      }
    }

    const sbomVersionDrift = [];
    collectSbomVersionDrift(
      JSON.parse(provenanceSbom),
      expectedPurls,
      expectedVersion,
      sbomVersionDrift,
    );
    for (const drift of sbomVersionDrift) {
      results.push(`${meta.provenanceSbomPath}: ${drift}`);
    }
  }

  return results;
}

function syncVersion(nextVersion) {
  const meta = packageMeta();
  const platforms = platformPackageManifests();
  const previousVersion = meta.root.version;

  writeText(repoPath("VERSION"), `${nextVersion}\n`);
  meta.root.version = nextVersion;
  if (meta.root.optionalDependencies) {
    const platformPackageNames = new Set(
      platforms.map(({ manifest }) => manifest.name),
    );
    const retainedOptionalDependencies = Object.fromEntries(
      Object.entries(meta.root.optionalDependencies).filter(
        ([packageName]) =>
          !platformPackageNames.has(packageName) &&
          !packageName.startsWith(meta.optionalPrefix),
      ),
    );
    if (
      Object.keys(retainedOptionalDependencies).length > 0
    ) {
      meta.root.optionalDependencies =
        retainedOptionalDependencies;
    } else {
      delete meta.root.optionalDependencies;
    }
  }
  writeJson(meta.packageJsonPath, meta.root);

  const wasmManifest = readJson(meta.wasmManifestPath);
  wasmManifest.version = nextVersion;
  writeJson(meta.wasmManifestPath, wasmManifest);

  for (const { manifestPath, manifest } of platforms) {
    manifest.version = nextVersion;
    writeJson(manifestPath, manifest);
  }

  const cargoTomlPaths = workspaceCargoTomlPaths();
  const cargoNames = cargoPackageNames(cargoTomlPaths);
  for (const cargoTomlPath of cargoTomlPaths) {
    let cargoToml = readText(cargoTomlPath);
    cargoToml = replaceRequired(
      cargoToml,
      /^version = "([^"]+)"$/m,
      `version = "${nextVersion}"`,
      cargoTomlPath,
    );
    for (const cargoName of cargoNames) {
      cargoToml = replaceIfPresent(
        cargoToml,
        new RegExp(
          `^(${escapeRegex(cargoName)} = \\{[^\\n]*version = ")[^"]+(")`,
          "m",
        ),
        `$1${nextVersion}$2`,
      );
    }
    writeText(cargoTomlPath, cargoToml);
  }

  let cargoLock = readText(meta.cargoLockPath);
  for (const cargoName of cargoNames) {
    cargoLock = replaceRequired(
      cargoLock,
      new RegExp(
        `(\\[\\[package\\]\\]\\nname = "${escapeRegex(cargoName)}"\\nversion = ")[^"]+(")`,
      ),
      `$1${nextVersion}$2`,
      meta.cargoLockPath,
    );
  }
  writeText(meta.cargoLockPath, cargoLock);

  if (fileExists(meta.indexCjsPath)) {
    let indexCjs = readText(meta.indexCjsPath);
    if (!indexCjs.includes(previousVersion)) {
      throw new Error(
        `Expected ${meta.indexCjsPath} to contain ${previousVersion}`,
      );
    }
    indexCjs = indexCjs.replaceAll(
      previousVersion,
      nextVersion,
    );
    writeText(meta.indexCjsPath, indexCjs);
  }

  if (fileExists(meta.provenanceSbomPath)) {
    let provenanceSbom = readText(meta.provenanceSbomPath);
    for (const prefix of npmPurlPrefixes(meta.root.name)) {
      provenanceSbom = replaceIfPresent(
        provenanceSbom,
        new RegExp(`${escapeRegex(prefix)}[^"\\s<]+`, "g"),
        `${prefix}${nextVersion}`,
      );
    }
    for (const cargoName of cargoNames) {
      provenanceSbom = replaceRequired(
        provenanceSbom,
        new RegExp(
          `pkg:cargo/${escapeRegex(cargoName)}@[^"\\s<]+`,
          "g",
        ),
        `pkg:cargo/${cargoName}@${nextVersion}`,
        meta.provenanceSbomPath,
      );
    }
    for (const purl of [
      ...npmPurlCandidates(meta.root.name, nextVersion),
      ...cargoNames.map(
        (cargoName) => `pkg:cargo/${cargoName}@${nextVersion}`,
      ),
    ]) {
      provenanceSbom = replaceSbomVersionForPurl(
        provenanceSbom,
        purl,
        nextVersion,
      );
    }
    for (const cargoName of cargoNames) {
      provenanceSbom = replaceIfPresent(
        provenanceSbom,
        new RegExp(
          `("purl": "pkg:cargo/${escapeRegex(cargoName)}@${escapeRegex(nextVersion)}",\\n\\s+"type": "[^"]+",\\n\\s+"version": ")[^"]+(")`,
          "g",
        ),
        `$1${nextVersion}$2`,
      );
    }
    writeText(meta.provenanceSbomPath, provenanceSbom);
  }
}

function parseArgs() {
  const [command, ...rest] = process.argv.slice(2);
  const args = new Map();
  for (let i = 0; i < rest.length; i += 1) {
    const token = rest[i];
    if (token === "--version") {
      const value = rest[i + 1];
      if (value == null) {
        throw new Error("Missing value for --version");
      }
      args.set("version", value);
      i += 1;
      continue;
    }
    if (token === "--tag") {
      const value = rest[i + 1];
      if (value == null) {
        throw new Error("Missing value for --tag");
      }
      args.set("tag", value);
      i += 1;
      continue;
    }
    throw new Error(`Unknown argument: ${token}`);
  }
  return { command, args };
}

function main() {
  const { command, args } = parseArgs();

  if (command !== "sync" && command !== "check") {
    console.error(
      "Usage: node scripts/version-sync.mjs <sync|check> [--version <semver>] [--tag <git-tag>]",
    );
    process.exit(1);
  }

  const version =
    args.get("version") ??
    args.get("tag")?.replace(/^v/, "") ??
    readVersion();

  if (command === "sync") {
    syncVersion(version);
    return;
  }

  if (command === "check") {
    const drift = mismatches(version);
    if (drift.length > 0) {
      console.error("Version drift detected:");
      for (const mismatch of drift) {
        console.error(`- ${mismatch}`);
      }
      process.exit(1);
    }
    return;
  }
}

main();
