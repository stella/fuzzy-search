import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

import { at, list, mapping, parseJson, text } from "./structured";

const root = new URL("../", import.meta.url);
const parseWorkflow = (path: string): unknown =>
  Bun.YAML.parse(readFileSync(new URL(path, root), "utf8"));
const steps = (workflow: unknown, job: string) =>
  list(at(workflow, "jobs", job, "steps"), `${job} steps`).map((step) =>
    mapping(step, `${job} step`),
  );

test("the aggregate CI result depends on every validation job", () => {
  const workflow = parseWorkflow(".github/workflows/ci.yml");
  const jobs = mapping(at(workflow, "jobs"), "jobs");
  const aggregate = mapping(jobs["ci-result"], "ci-result");
  const validationJobs = Object.keys(jobs).filter(
    (name) => name !== "ci-result",
  );
  const needs = list(aggregate["needs"], "ci-result needs").map((need) =>
    text(need, "ci-result need"),
  );

  expect(aggregate["if"]).toBe("always()");
  expect(needs.sort((left, right) => left.localeCompare(right))).toEqual(
    validationJobs.sort((left, right) => left.localeCompare(right)),
  );
});

test("CI and CLA workflows handle merge queue checks", () => {
  for (const path of [
    ".github/workflows/ci.yml",
    ".github/workflows/cla.yml",
  ]) {
    const workflow = parseWorkflow(path);
    expect(at(workflow, "on", "merge_group", "types")).toEqual([
      "checks_requested",
    ]);
  }
});

test("CI runs the aggregate result regression tests", () => {
  const workflow = parseWorkflow(".github/workflows/ci.yml");
  expect(
    steps(workflow, "checks").some(
      (step) =>
        typeof step["run"] === "string" &&
        step["run"].includes("scripts/ci-result.test.ts"),
    ),
  ).toBeTrue();
});

test("release artifacts retain the complete NAPI WASI loader bundle", () => {
  const workflow = parseWorkflow(".github/workflows/release.yml");
  const upload = steps(workflow, "build").find(
    (step) =>
      typeof step["uses"] === "string" &&
      step["uses"].startsWith("actions/upload-artifact@"),
  );
  const path = upload === undefined ? undefined : at(upload, "with", "path");
  if (typeof path !== "string") {
    throw new Error("Release upload must declare artifact paths");
  }
  const patterns = path.trim().split(/\s+/u);
  const selected = (file: string) =>
    patterns.some(
      (pattern) =>
        !pattern.startsWith("!") && new Bun.Glob(pattern).match(file),
    ) &&
    !patterns.some(
      (pattern) =>
        pattern.startsWith("!") && new Bun.Glob(pattern.slice(1)).match(file),
    );
  const manifest = parseJson(
    readFileSync(new URL("package.json", root), "utf8"),
  );
  const binary = text(at(manifest, "napi", "binaryName"), "napi.binaryName");
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
