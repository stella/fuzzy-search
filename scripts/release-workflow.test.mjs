import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const workflow = readFileSync(
  new URL("../.github/workflows/release.yml", import.meta.url),
  "utf8",
).replaceAll("\r\n", "\n");

const readJob = (name) => {
  const marker = `  ${name}:\n`;
  const start = workflow.indexOf(marker);
  expect(start).not.toBe(-1);

  const bodyStart = start + marker.length;
  const remainder = workflow.slice(bodyStart);
  const nextJob = remainder.search(/^  [A-Za-z_][A-Za-z0-9_-]*:\n/m);
  return nextJob === -1
    ? workflow.slice(start)
    : workflow.slice(start, bodyStart + nextJob);
};

describe("Rust release ordering", () => {
  test("separates verified releases from crate-only recovery", () => {
    const publish = readJob("publish-core");
    const recovery = readJob("recover-core");

    expect(publish).toContain(
      "needs: [preflight, core-preflight, verify, pack, attest]",
    );
    expect(publish).toContain(
      "needs.preflight.outputs.already-released != 'true'",
    );
    expect(publish).toContain("needs.pack.result == 'success'");
    expect(publish).toContain("needs.attest.result == 'success'");
    expect(recovery).toContain("needs: [preflight, core-preflight, verify]");
    expect(recovery).toContain(
      "needs.preflight.outputs.already-released == 'true'",
    );
    expect(recovery).toContain(
      "needs.core-preflight.outputs.already-released != 'true'",
    );
    expect(publish).not.toMatch(/always\(\)|failure\(\)|cancelled\(\)/);
    expect(recovery).not.toMatch(/always\(\)|failure\(\)|cancelled\(\)/);
  });
});

test("privileged shared publishers match the release policy pin", () => {
  const policy = readFileSync(
    new URL("../.github/workflows/release-policy.yml", import.meta.url),
    "utf8",
  );
  const policyUse = policy.match(
    /uses: stella\/\.github\/\.github\/workflows\/release-policy\.yml@([0-9a-f]{40})/u,
  );
  const pin = policyUse?.at(1);
  if (pin === undefined)
    throw new Error("Missing immutable release policy pin");
  const publishers = [
    ...workflow.matchAll(
      /uses: stella\/\.github\/\.github\/workflows\/(?:crates-io-publish|npm-version-finalize)\.yml@([0-9a-f]{40})/gu,
    ),
  ];
  expect(publishers).toHaveLength(3);
  for (const publisher of publishers) expect(publisher.at(1)).toBe(pin);
});

test("the release reconciler checks the packages the release publishes", () => {
  const reconcile = readFileSync(
    new URL("../.github/workflows/npm-release-reconcile.yml", import.meta.url),
    "utf8",
  ).replaceAll("\r\n", "\n");
  const packageFiles = (source) =>
    [...source.matchAll(/package-files: \|\n((?: {8}\S.*\n)+)/gu)].map(
      ([, list]) => list,
    );
  const [reconciled, ...extra] = packageFiles(reconcile);
  expect(reconciled).toBeDefined();
  expect(extra).toEqual([]);
  // Preflight and the finalizer.
  const released = packageFiles(workflow);
  expect(released).toHaveLength(2);
  for (const list of released) expect(list).toBe(reconciled);
  const pin = reconcile.match(
    /uses: stella\/\.github\/\.github\/workflows\/npm-release-reconcile\.yml@([0-9a-f]{40})\n/u,
  )?.[1];
  const finalizerPin = workflow.match(
    /uses: stella\/\.github\/\.github\/workflows\/npm-version-finalize\.yml@([0-9a-f]{40})\n/u,
  )?.[1];
  expect(pin).toBeDefined();
  expect(pin).toBe(finalizerPin);
  const releaseAppSecrets = (source) =>
    [
      ...source.matchAll(
        /^ {6}(RELEASE_APP_(?:ID|PRIVATE_KEY)): (\$\{\{ secrets\.[A-Z0-9_]+ \}\})$/gmu,
      ),
    ].map(([, name, value]) => `${name}=${value}`);
  expect(releaseAppSecrets(reconcile)).toHaveLength(2);
  expect(releaseAppSecrets(reconcile)).toEqual(
    releaseAppSecrets(readJob("finalize")),
  );
});

test("the WASI build artifact uploads the root loader its metadata names", () => {
  const read = (path) =>
    readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
  const prefix = "// napi-rs-artifact-metadata:";
  const [firstLine] = read("fuzzy-search.wasi.cjs").split("\n", 1);
  expect(firstLine.startsWith(prefix)).toBe(true);
  const rootEntry = String(
    JSON.parse(firstLine.slice(prefix.length)).rootEntry,
  );
  // `napi artifacts` copies this file out of the WASI build artifact by name,
  // so the build must emit it under that name and the upload must keep it.
  expect(rootEntry).toBe("index.cjs");
  expect(JSON.parse(read("package.json")).files).toContain(rootEntry);
  expect(read("scripts/build-native.mjs")).toMatch(/"--js",\s*"index\.cjs",/u);

  const uploads = Bun.YAML.parse(workflow).jobs.build.steps.filter((step) =>
    String(step.uses).startsWith("actions/upload-artifact@"),
  );
  expect(uploads).toHaveLength(1);
  const patterns = String(uploads[0].with.path).trim().split(/\s+/u);
  const matches = (pattern) => new Bun.Glob(pattern).match(rootEntry);
  expect(
    patterns.some((pattern) => !pattern.startsWith("!") && matches(pattern)),
  ).toBe(true);
  expect(
    patterns.some(
      (pattern) => pattern.startsWith("!") && matches(pattern.slice(1)),
    ),
  ).toBe(false);
});
