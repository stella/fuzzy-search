import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const readWorkflow = (name: string) =>
  readFileSync(
    new URL(`../.github/workflows/${name}.yml`, import.meta.url),
    "utf8",
  );

test("shared typecheck retains tooling invariants without duplicate compiler checks", () => {
  const workflow = readWorkflow("ci");
  expect(workflow).toContain("      typecheck: true");
  expect(workflow).toContain("__test__/tooling-adoption.spec.ts");
  expect(workflow).not.toContain("- run: bun run typecheck");
  expect(workflow).not.toContain("- run: bun run check:typecheck-parity");
});

test("nightly parity uses the same shared workflow and runs only compiler checks", () => {
  const nightly = readWorkflow("typecheck-nightly");
  const shared =
    /uses: (stella\/.github\/.github\/workflows\/rust-napi-ci.yml@\w+)/;
  expect(nightly.match(shared)?.at(1)).toBe(
    readWorkflow("ci").match(shared)?.at(1),
  );
  expect(nightly).toContain("      typecheck: true");
  expect(nightly).toContain("      typecheck-only: true");
  expect(nightly).toContain("      parity: always");
});
