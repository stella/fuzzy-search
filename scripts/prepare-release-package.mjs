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

function readJson(filePath) {
  return JSON.parse(readText(filePath));
}

function writeJson(filePath, value) {
  fs.writeFileSync(
    filePath,
    `${JSON.stringify(value, null, 2)}\n`,
  );
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

function platformPackages() {
  const npmDir = repoPath("npm");
  return fs
    .readdirSync(npmDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => {
      const packageJsonPath = path.join(
        npmDir,
        entry.name,
        "package.json",
      );
      return {
        packageJsonPath,
        manifest: readJson(packageJsonPath),
      };
    })
    .sort((left, right) =>
      left.manifest.name.localeCompare(right.manifest.name),
    );
}

const version = readText(repoPath("VERSION")).trim();
const packageJsonPath = repoPath("package.json");
const root = readJson(packageJsonPath);
const [scope, baseName] = root.name.split("/");
if (!scope || !baseName) {
  fail(`Expected scoped package name, got ${root.name}`);
}
if (root.version !== version) {
  fail(
    `${packageJsonPath}: version=${root.version}; expected ${version}`,
  );
}

const optionalPrefix = `${scope}/${baseName}-`;
const generatedOptionalDependencies = {};
for (const {
  packageJsonPath: manifestPath,
  manifest,
} of platformPackages()) {
  if (manifest.version !== version) {
    fail(
      `${manifestPath}: version=${manifest.version}; expected ${version}`,
    );
  }
  if (!manifest.name.startsWith(optionalPrefix)) {
    fail(
      `${manifestPath}: unexpected package name ${manifest.name}`,
    );
  }
  generatedOptionalDependencies[manifest.name] = version;
}

const existingOptionalDependencies = Object.fromEntries(
  Object.entries(root.optionalDependencies ?? {}).filter(
    ([name]) => !name.startsWith(optionalPrefix),
  ),
);
root.optionalDependencies = sortObject({
  ...existingOptionalDependencies,
  ...generatedOptionalDependencies,
});

writeJson(packageJsonPath, root);
