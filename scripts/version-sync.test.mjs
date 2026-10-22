import { describe, expect, test } from "bun:test";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { replaceLoaderVersion, resolveVersion } from "./version-sync.mjs";

describe("Changesets version synchronization", () => {
  test("sync follows the package version while checks follow VERSION", () => {
    const versions = {
      explicitVersion: undefined,
      packageVersion: "1.1.4",
      tag: undefined,
      versionFileVersion: "1.1.3",
    };

    expect(resolveVersion({ command: "sync", ...versions })).toBe("1.1.4");
    expect(resolveVersion({ command: "check", ...versions })).toBe("1.1.3");
  });

  test("updates the generated loader from VERSION to the package version", () => {
    const content = "bindingPackageVersion !== '1.1.3'; expected 1.1.3 but got";

    expect(
      replaceLoaderVersion({
        content,
        filePath: "index.cjs",
        sourceVersion: "1.1.3",
        targetVersion: "1.1.4",
      }),
    ).toBe("bindingPackageVersion !== '1.1.4'; expected 1.1.4 but got");
  });
});

test("version check accepts the checked-in CycloneDX SBOM and detects nested component drift", () => {
  const root = realpathSync(
    mkdtempSync(path.join(tmpdir(), "fuzzy-version-check-")),
  );
  /** @param {string} relative */
  const copy = (relative) => {
    const destination = path.join(root, relative);
    mkdirSync(path.dirname(destination), {
      recursive: true,
    });
    copyFileSync(new URL(`../${relative}`, import.meta.url), destination);
  };
  try {
    for (const relative of [
      "VERSION",
      "package.json",
      "Cargo.toml",
      "Cargo.lock",
      "bun.lock",
      "index.cjs",
      "scripts/version-sync.mjs",
      "scripts/release-json.mjs",
      "wasm/package.json",
      "crates/core/Cargo.toml",
      "provenance/sbom.cdx.json",
    ])
      copy(relative);
    for (const entry of readdirSync(new URL("../npm/", import.meta.url), {
      withFileTypes: true,
    })) {
      if (entry.isDirectory()) copy(`npm/${entry.name}/package.json`);
    }
    const run = () =>
      Bun.spawnSync(
        ["node", path.join(root, "scripts/version-sync.mjs"), "check"],
        { cwd: root },
      );
    const valid = run();
    expect(valid.stderr.toString()).toBe("");
    expect(valid.exitCode).toBe(0);
    const sbomPath = path.join(root, "provenance/sbom.cdx.json");
    const checkedInSbom = readFileSync(sbomPath, "utf8");
    const version = readFileSync(path.join(root, "VERSION"), "utf8").trim();
    const purl = `pkg:cargo/stella-fuzzy-search-core@${version}`;
    for (const componentVersion of ["0.0.0", { unexpected: true }]) {
      writeFileSync(
        sbomPath,
        JSON.stringify({
          checkedInSbom: JSON.parse(checkedInSbom),
          nested: [{ purl, version: componentVersion }],
        }),
      );
      const drift = run();
      expect(drift.exitCode).toBe(1);
      expect(drift.stderr.toString()).toContain(
        `sbom.cdx.json: ${purl}: version=${typeof componentVersion === "string" ? componentVersion : JSON.stringify(componentVersion)}`,
      );
      expect(drift.stderr.toString()).not.toContain(
        "Expected package name and version strings",
      );
      expect(drift.stderr.toString()).not.toContain("[object Object]");
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
