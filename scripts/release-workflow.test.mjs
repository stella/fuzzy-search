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
