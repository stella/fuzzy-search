import { expect, test } from "bun:test";

import {
  formatSbomVersion,
  parseCargoMetadata,
  parsePackageManifest,
  parseRegistryVersion,
} from "./release-json.mjs";

test("package metadata narrows strings and retains unrelated fields", () => {
  const manifest = {
    name: "@stll/example",
    version: "1.2.3",
    main: "binding.node",
    optionalDependencies: { native: "1.2.3" },
    custom: { enabled: true },
  };
  expect(
    parsePackageManifest(JSON.stringify(manifest)),
  ).toEqual(manifest);
  for (const invalid of [
    null,
    [],
    {},
    { ...manifest, name: 1 },
    { ...manifest, version: {} },
    { ...manifest, main: 1 },
    { ...manifest, optionalDependencies: { native: 1 } },
  ])
    expect(() =>
      parsePackageManifest(JSON.stringify(invalid)),
    ).toThrow();
});

test("Cargo and registry envelopes validate fields before release decisions", () => {
  const metadata = {
    target_directory: "target",
    packages: [
      { name: "private", version: "1.2.3", publish: [] },
      {
        name: "public",
        version: "1.2.3",
        publish: ["crates-io"],
      },
      { name: "default", version: "1.2.3", publish: null },
    ],
  };
  expect(parseCargoMetadata(metadata)).toEqual(metadata);
  for (const invalid of [
    null,
    {},
    { ...metadata, packages: [null] },
    {
      ...metadata,
      packages: [
        { name: "public", version: "1.2.3", publish: [1] },
      ],
    },
  ])
    expect(() => parseCargoMetadata(invalid)).toThrow();
  expect(
    parseRegistryVersion({ version: { checksum: "abc" } }),
  ).toEqual({ checksum: "abc" });
  for (const invalid of [
    null,
    {},
    { version: null },
    { version: { checksum: 1 } },
  ])
    expect(() => parseRegistryVersion(invalid)).toThrow();
});

test("SBOM drift diagnostics preserve structured invalid versions without object coercion", () => {
  expect(formatSbomVersion("1.2.3")).toBe("1.2.3");
  for (const missing of [null, undefined])
    expect(formatSbomVersion(missing)).toBe("<missing>");
  for (const value of [
    { version: "unexpected" },
    ["unexpected"],
    false,
    4,
  ]) {
    expect(formatSbomVersion(value)).toBe(
      JSON.stringify(value),
    );
    expect(formatSbomVersion(value)).not.toContain(
      "[object Object]",
    );
  }
});
