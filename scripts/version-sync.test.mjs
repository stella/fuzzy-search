import { describe, expect, test } from "bun:test";

import {
  replaceLoaderVersion,
  resolveVersion,
} from "./version-sync.mjs";

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
    const content =
      "bindingPackageVersion !== '1.1.3'; expected 1.1.3 but got";

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
