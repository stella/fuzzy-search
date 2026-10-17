import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const readWorkflow = (name: string) =>
  readFileSync(new URL(`../.github/workflows/${name}.yml`, import.meta.url), "utf8");

test("optional parity is dispatch-only and retains toolchain verification", () => {
  const workflow = readWorkflow("optional-proofs");
  expect(workflow).toContain("  workflow_dispatch:");
  expect(workflow).not.toMatch(/^  (push|pull_request|schedule):/m);
  expect(workflow).toContain("bun install --frozen-lockfile");
  expect(workflow).toContain("bun run check:toolchain");
  expect(workflow).toContain("bun run check:typecheck-parity");
});

test("automatic parity stays conditional on toolchain changes", () => {
  expect(readWorkflow("ci")).toContain(
    "- run: bun run check:typecheck-parity\n        if: needs.check-changes.outputs.toolchain-changed == 'true'",
  );
});
