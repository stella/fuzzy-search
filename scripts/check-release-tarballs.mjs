#!/usr/bin/env node

import { execFileSync } from "node:child_process";
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

function readJson(filePath) {
  return JSON.parse(readText(filePath));
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

function sortObject(value) {
  return Object.fromEntries(
    Object.entries(value).sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  );
}

function packageFromTarball(tarball) {
  const manifest = execFileSync(
    "tar",
    ["-xOf", tarball, "package/package.json"],
    { encoding: "utf8" },
  );
  return JSON.parse(manifest);
}

function platformManifests() {
  const npmDir = repoPath("npm");
  return fs
    .readdirSync(npmDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) =>
      readJson(
        path.join(npmDir, entry.name, "package.json"),
      ),
    )
    .sort((left, right) =>
      left.name.localeCompare(right.name),
    );
}

const [rootTarball, ...auxTarballs] = process.argv.slice(2);
if (!rootTarball || auxTarballs.length === 0) {
  fail(
    "Usage: node scripts/check-release-tarballs.mjs <root.tgz> <aux.tgz...>",
  );
}

const version = readText(repoPath("VERSION")).trim();
const sourceRoot = readJson(repoPath("package.json"));
const sourceWasm = readJson(
  repoPath("wasm", "package.json"),
);
const platforms = platformManifests();
const platformPackageNames = new Set(
  platforms.map((manifest) => manifest.name),
);
const sourceOptionalDependencies = Object.fromEntries(
  Object.entries(sourceRoot.optionalDependencies ?? {}).filter(
    ([name]) => !platformPackageNames.has(name),
  ),
);
const expectedOptionalDependencies = sortObject(
  {
    ...sourceOptionalDependencies,
    ...Object.fromEntries(
      platforms.map((manifest) => [manifest.name, version]),
    ),
  },
);

const root = packageFromTarball(rootTarball);
if (root.name !== sourceRoot.name) {
  fail(
    `${rootTarball}: name=${root.name}; expected ${sourceRoot.name}`,
  );
}
if (root.version !== version) {
  fail(
    `${rootTarball}: version=${root.version}; expected ${version}`,
  );
}
if (
  JSON.stringify(
    sortObject(root.optionalDependencies ?? {}),
  ) !== JSON.stringify(expectedOptionalDependencies)
) {
  fail(
    `${rootTarball}: optionalDependencies do not match platform packages`,
  );
}

const expectedAuxiliaryNames = new Set([
  ...platformPackageNames,
  sourceWasm.name,
]);
const seenAuxiliaryNames = new Set();
for (const tarball of auxTarballs) {
  const manifest = packageFromTarball(tarball);
  if (!expectedAuxiliaryNames.has(manifest.name)) {
    fail(`${tarball}: unexpected package ${manifest.name}`);
  }
  if (seenAuxiliaryNames.has(manifest.name)) {
    fail(`${tarball}: duplicate package ${manifest.name}`);
  }
  if (manifest.version !== version) {
    fail(
      `${tarball}: version=${manifest.version}; expected ${version}`,
    );
  }
  seenAuxiliaryNames.add(manifest.name);
}

for (const name of [...expectedAuxiliaryNames].sort(
  (left, right) => left.localeCompare(right),
)) {
  if (!seenAuxiliaryNames.has(name)) {
    fail(`Missing release tarball for ${name}`);
  }
}
