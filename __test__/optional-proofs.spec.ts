import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const isRecord = (
  value: unknown,
): value is Record<string, unknown> =>
  typeof value === "object" &&
  value !== null &&
  !Array.isArray(value);

const narrowWorkflow = (value: unknown, name: string) => {
  if (
    !isRecord(value) ||
    !isRecord(value.on) ||
    !isRecord(value.jobs)
  ) {
    throw new Error(
      `${name}: expected workflow on/jobs mappings`,
    );
  }
  const jobs = value.jobs;
  return {
    on: value.on,
    jobNames: Object.keys(jobs),
    job: (id: string) => {
      const job = jobs[id];
      if (!isRecord(job))
        throw new Error(`${name}: missing job ${id}`);
      if (
        job.uses !== undefined &&
        typeof job.uses !== "string"
      ) {
        throw new Error(
          `${name}/${id}: expected string uses`,
        );
      }
      if (job.with !== undefined && !isRecord(job.with)) {
        throw new Error(
          `${name}/${id}: expected with mapping`,
        );
      }
      const steps = job.steps;
      if (steps !== undefined && !Array.isArray(steps)) {
        throw new Error(
          `${name}/${id}: expected steps array`,
        );
      }
      return {
        uses: job.uses,
        with: job.with,
        steps: (steps ?? []).map((step: unknown) => {
          if (!isRecord(step))
            throw new Error(
              `${name}/${id}: expected step mapping`,
            );
          if (
            step.run !== undefined &&
            typeof step.run !== "string"
          ) {
            throw new Error(
              `${name}/${id}: expected string run`,
            );
          }
          if (
            step.uses !== undefined &&
            typeof step.uses !== "string"
          ) {
            throw new Error(
              `${name}/${id}: expected string step uses`,
            );
          }
          if (
            step.with !== undefined &&
            !isRecord(step.with)
          ) {
            throw new Error(
              `${name}/${id}: expected step with mapping`,
            );
          }
          return {
            run: step.run,
            uses: step.uses,
            if: step.if,
            with: step.with,
          };
        }),
      };
    },
  };
};

const readWorkflow = (name: string) =>
  narrowWorkflow(
    Bun.YAML.parse(
      readFileSync(
        new URL(
          `../.github/workflows/${name}.yml`,
          import.meta.url,
        ),
        "utf8",
      ),
    ),
    name,
  );

test("optional parity is dispatch-only and retains toolchain verification", () => {
  const workflow = readWorkflow("optional-proofs");
  expect(Object.keys(workflow.on)).toEqual([
    "workflow_dispatch",
  ]);
  const steps = workflow.job("typecheck-parity").steps;
  for (const command of [
    "bun install --frozen-lockfile",
    "bun run check:toolchain",
    "bun run check:typecheck-parity",
  ]) {
    const step = steps.find(
      (candidate) => candidate.run === command,
    );
    expect(step).toBeDefined();
    expect(step?.if).toBeUndefined();
  }
});

test("automatic compiler checks use shared CI without duplicate parity steps", () => {
  const workflow = readWorkflow("ci");
  expect(Object.keys(workflow.on).sort()).toEqual([
    "pull_request",
    "push",
    "workflow_dispatch",
  ]);
  expect(workflow.job("napi-ci").with?.typecheck).toBe(
    true,
  );
  const parity = workflow
    .job("checks")
    .steps.find(
      (step) =>
        step.run === "bun run check:typecheck-parity",
    );
  expect(parity).toBeUndefined();
});

test("workflow views reject malformed YAML shapes with a useful location", () => {
  for (const value of [
    null,
    [],
    { on: [], jobs: {} },
    { on: {}, jobs: null },
  ]) {
    expect(() => narrowWorkflow(value, "fixture")).toThrow(
      "fixture: expected workflow on/jobs mappings",
    );
  }
  for (const job of [
    null,
    { uses: 1 },
    { with: [] },
    { steps: "run" },
    { steps: [null] },
    { steps: [{ run: 1 }] },
    { steps: [{ uses: false }] },
    { steps: [{ with: [] }] },
  ]) {
    expect(() =>
      narrowWorkflow(
        { on: {}, jobs: { proofs: job } },
        "fixture",
      ).job("proofs"),
    ).toThrow("fixture");
  }
  const shared = narrowWorkflow(
    {
      on: {},
      jobs: { shared: { uses: "owner/repo/workflow@sha" } },
    },
    "fixture",
  );
  expect(shared.job("shared").steps).toEqual([]);
  expect(() => shared.job("missing")).toThrow(
    "fixture: missing job missing",
  );
});
