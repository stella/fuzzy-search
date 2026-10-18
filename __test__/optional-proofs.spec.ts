import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const readWorkflow = (name: string) =>
  Bun.YAML.parse(
    readFileSync(
      new URL(
        `../.github/workflows/${name}.yml`,
        import.meta.url,
      ),
      "utf8",
    ),
  );

test("optional parity is dispatch-only and retains toolchain verification", () => {
  const workflow = readWorkflow("optional-proofs");
  expect(Object.keys(workflow.on)).toEqual([
    "workflow_dispatch",
  ]);
  const steps = workflow.jobs["typecheck-parity"].steps;
  for (const command of [
    "bun install --frozen-lockfile",
    "bun run check:toolchain",
    "bun run check:typecheck-parity",
  ]) {
    const step = steps.find(
      (candidate: { run?: string }) =>
        candidate.run === command,
    );
    expect(step).toBeDefined();
    expect(step.if).toBeUndefined();
  }
});

test("automatic compiler checks use shared CI without duplicate parity steps", () => {
  const workflow = readWorkflow("ci");
  expect(Object.keys(workflow.on).sort()).toEqual([
    "pull_request",
    "push",
    "workflow_dispatch",
  ]);
  expect(workflow.jobs["napi-ci"].with.typecheck).toBe(
    true,
  );
  const parity = workflow.jobs.checks.steps.find(
    (step: { run?: string }) =>
      step.run === "bun run check:typecheck-parity",
  );
  expect(parity).toBeUndefined();
});
