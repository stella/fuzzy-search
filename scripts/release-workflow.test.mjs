import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const workflowPath =
  process.env.RELEASE_WORKFLOW_PATH ??
  ".github/workflows/release.yml";
const workflow = readFileSync(workflowPath, "utf8");

const parseJobs = (source) => {
  const jobsMarker = source.match(/^jobs:\n/m);
  assert.notEqual(
    jobsMarker,
    null,
    "workflow must declare jobs",
  );
  const jobsSource = source.slice(
    jobsMarker.index + jobsMarker[0].length,
  );
  const jobEntries = [
    ...jobsSource.matchAll(
      /^  (?:(?:"([A-Za-z_][A-Za-z0-9_-]*)")|(?:'([A-Za-z_][A-Za-z0-9_-]*)')|([A-Za-z_][A-Za-z0-9_-]*)):\n/gm,
    ),
  ].map((match, index, matches) => {
    const next = matches.at(index + 1);
    return [
      match[1] ?? match[2] ?? match[3],
      jobsSource.slice(
        match.index,
        next?.index ?? jobsSource.length,
      ),
    ];
  });
  return Object.fromEntries(jobEntries);
};
const jobs = parseJobs(workflow);

const privilegedJobNamesFor = (jobMap) =>
  Object.entries(jobMap)
    .filter(([, job]) =>
      /^      id-token: write$/m.test(job),
    )
    .map(([name]) => name)
    .sort();
const privilegedJobNames = privilegedJobNamesFor(jobs);

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

const assertNoForbiddenPrivilegedCommands = (jobMap) => {
  for (const name of privilegedJobNamesFor(jobMap)) {
    const job = jobMap[name];
    for (const forbidden of forbiddenPrivilegedCommands) {
      assert.doesNotMatch(
        job,
        forbidden,
        `${name} must not match ${forbidden}`,
      );
    }
  }
};

void test("OIDC jobs only consume prepared release artifacts", () => {
  assert.deepEqual(privilegedJobNames, [
    "attest",
    "core-recovery-attest",
    "finalize",
    "publish-core",
  ]);

  assertNoForbiddenPrivilegedCommands(jobs);
});

void test("all valid GitHub job IDs remain inside the OIDC guard", () => {
  const syntheticJobs = parseJobs(`jobs:
  _:
    permissions:
      id-token: write
    steps:
      - uses: actions/checkout@0123456789012345678901234567890123456789
  a:
    permissions:
      id-token: write
    steps: []
  Upper_case-2:
    steps: []
  "_quoted":
    permissions:
      id-token: write
    steps: []
`);

  assert.deepEqual(Object.keys(syntheticJobs), [
    "_",
    "a",
    "Upper_case-2",
    "_quoted",
  ]);
  assert.deepEqual(privilegedJobNamesFor(syntheticJobs), [
    "_",
    "_quoted",
    "a",
  ]);
  assert.throws(
    () =>
      assertNoForbiddenPrivilegedCommands(syntheticJobs),
    /_ must not match \/actions\\\/checkout@\//,
  );
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

void test("crate publication never retries PUT and proves the exact published bytes", () => {
  const publishCore = jobs["publish-core"];

  assert.equal(
    publishCore.match(
      /https:\/\/crates\.io\/api\/v1\/crates\/new/g,
    )?.length,
    1,
  );
  assert.match(
    publishCore,
    /upload_status="\$\(curl[^\n]*--max-time 600[^\n]*\\/,
  );
  assert.doesNotMatch(
    publishCore,
    /upload_status="\$\(curl[^\n]*--retry/,
  );
  assert.doesNotMatch(
    publishCore,
    /^      && needs\.core-preflight\.outputs\.already-released != 'true'$/m,
  );
  assert.match(publishCore, /CRATE_CHECKSUM/);
  assert.match(publishCore, /sha256sum/);
  assert.match(
    publishCore,
    /ambiguous upload committed the exact crate bytes/,
  );
});
