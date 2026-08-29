import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const workflowPath =
  process.env.RELEASE_WORKFLOW_PATH ??
  ".github/workflows/release.yml";
const workflow = readFileSync(workflowPath, "utf8");

const jobEntries = [
  ...workflow.matchAll(/^  ([a-z][a-z0-9-]*):\n/gm),
].map((match, index, matches) => {
  const next = matches.at(index + 1);
  return [
    match[1],
    workflow.slice(
      match.index,
      next?.index ?? workflow.length,
    ),
  ];
});
const jobs = Object.fromEntries(jobEntries);

const privilegedJobNames = Object.entries(jobs)
  .filter(([, job]) => /^      id-token: write$/m.test(job))
  .map(([name]) => name)
  .sort();

const forbiddenPrivilegedCommands = [
  /actions\/checkout@/,
  /actions\/setup-node@/,
  /dtolnay\/rust-toolchain@/,
  /oven-sh\/setup-bun@/,
  /bun run build/,
  /cargo (?:build|install|package|publish|test)/,
  /(?:bun|npm|pnpm|yarn) (?:ci|install)/,
  /npm pack/,
];

void test("OIDC jobs only consume prepared release artifacts", () => {
  assert.deepEqual(privilegedJobNames, [
    "attest",
    "core-recovery-attest",
    "finalize",
    "publish-core",
  ]);

  for (const name of privilegedJobNames) {
    const job = jobs[name];
    for (const forbidden of forbiddenPrivilegedCommands) {
      assert.doesNotMatch(
        job,
        forbidden,
        `${name} must not match ${forbidden}`,
      );
    }
  }
});

void test("packaging and recovery hand exact artifacts to privileged jobs", () => {
  assert.doesNotMatch(jobs.pack, /id-token: write/);
  assert.match(jobs.pack, /name: release-artifacts/);
  assert.match(jobs.attest, /needs: \[pack\]/);
  assert.match(jobs.attest, /name: release-artifacts/);

  assert.doesNotMatch(
    jobs["core-recovery-package"],
    /id-token: write/,
  );
  assert.match(
    jobs["core-recovery-package"],
    /name: core-recovery-artifacts/,
  );
  assert.match(
    jobs["core-recovery-attest"],
    /name: core-recovery-artifacts/,
  );
  assert.match(
    jobs["publish-core"],
    /name: release-artifacts/,
  );
  assert.match(
    jobs["publish-core"],
    /name: core-recovery-artifacts/,
  );
  assert.match(
    jobs["publish-core"],
    /name: core-publish-metadata/,
  );
  assert.match(
    jobs["publish-core"],
    /https:\/\/crates\.io\/api\/v1\/crates\/new/,
  );
});
