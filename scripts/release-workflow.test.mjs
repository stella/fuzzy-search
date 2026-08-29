import { readFileSync } from "node:fs";
import { describe, expect, test } from "bun:test";

const workflow = readFileSync(
  new URL("../.github/workflows/release.yml", import.meta.url),
  "utf8",
);

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

    expect(publish).toContain("needs: [preflight, core-preflight, verify, pack, attest]");
    expect(publish).toContain("needs.preflight.outputs.already-released != 'true'");
    expect(publish).toContain("needs.pack.result == 'success'");
    expect(publish).toContain("needs.attest.result == 'success'");
    expect(recovery).toContain("needs: [preflight, core-preflight, verify]");
    expect(recovery).toContain("needs.preflight.outputs.already-released == 'true'");
    expect(recovery).toContain("needs.core-preflight.outputs.already-released != 'true'");
    expect(publish).not.toMatch(/always\(\)|failure\(\)|cancelled\(\)/);
    expect(recovery).not.toMatch(/always\(\)|failure\(\)|cancelled\(\)/);
  });
});
