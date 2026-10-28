import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const root = new URL("../", import.meta.url);
const parseWorkflow = (path: string) =>
  Bun.YAML.parse(readFileSync(new URL(path, root), "utf8"));

test("the aggregate CI result depends on every validation job", () => {
  const workflow = parseWorkflow(".github/workflows/ci.yml");
  const jobs = workflow.jobs;
  const aggregate = jobs["ci-result"];
  const validationJobs = Object.keys(jobs).filter(
    (name) => name !== "ci-result",
  );

  expect(aggregate.if).toBe("always()");
  expect(
    [...aggregate.needs].sort((left, right) => left.localeCompare(right)),
  ).toEqual(validationJobs.sort((left, right) => left.localeCompare(right)));
});

test("CI and CLA workflows handle merge queue checks", () => {
  for (const path of [
    ".github/workflows/ci.yml",
    ".github/workflows/cla.yml",
  ]) {
    const workflow = parseWorkflow(path);
    expect(workflow.on.merge_group.types).toEqual(["checks_requested"]);
  }
});

test("CI runs the aggregate result regression tests", () => {
  const workflow = parseWorkflow(".github/workflows/ci.yml");
  expect(
    workflow.jobs.checks.steps.some((step: { run?: string }) =>
      step.run?.includes("scripts/ci-result.test.ts"),
    ),
  ).toBeTrue();
});

test("release artifacts retain the complete NAPI WASI loader bundle", () => {
  const workflow = parseWorkflow(".github/workflows/release.yml");
  const upload = workflow.jobs.build.steps.find((step: { uses?: string }) =>
    step.uses?.startsWith("actions/upload-artifact@"),
  );
  const patterns = upload.with.path.trim().split(/\s+/u);
  const selected = (file: string) =>
    patterns.some(
      (pattern: string) =>
        !pattern.startsWith("!") && new Bun.Glob(pattern).match(file),
    ) &&
    !patterns.some(
      (pattern: string) =>
        pattern.startsWith("!") && new Bun.Glob(pattern.slice(1)).match(file),
    );
  const manifest = JSON.parse(
    readFileSync(new URL("package.json", root), "utf8"),
  );
  const binary = manifest.napi.binaryName;
  for (const file of [
    `${binary}.wasm32-wasi.wasm`,
    `${binary}.wasi.cjs`,
    `${binary}.wasi.d.cts`,
    `${binary}.wasi-browser.js`,
    "wasi-worker.mjs",
    "wasi-worker-browser.mjs",
  ]) {
    expect(selected(file), `artifact bundle omits ${file}`).toBeTrue();
  }
  expect(selected(`${binary}.debug.wasm`)).toBeFalse();
});
