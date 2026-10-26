import { expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";

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

test("no workflow runs a separate compiler parity check", () => {
  expect(
    existsSync(
      new URL("../.github/workflows/typecheck-nightly.yml", import.meta.url),
    ),
  ).toBe(false);
  const workflow = readWorkflow("ci");
  expect(workflow).not.toContain("typecheck-only:");
  expect(workflow).not.toContain("parity:");
});
