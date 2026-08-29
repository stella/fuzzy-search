import { YAML } from "bun";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const JOB_ID = /^[A-Za-z_][A-Za-z0-9_-]*$/;

const isRecord = (value) =>
  typeof value === "object" &&
  value !== null &&
  !Array.isArray(value);

const stepRunFingerprint = ({ name, run }) => ({
  name,
  sha256: createHash("sha256").update(run).digest("hex"),
});

const fingerprint = (value) =>
  createHash("sha256")
    .update(JSON.stringify(value))
    .digest("hex");

export const parseJobBodies = (workflow) => {
  const parsedWorkflow = YAML.parse(workflow);
  assert(
    isRecord(parsedWorkflow),
    "release workflow is not a mapping",
  );
  assert(
    isRecord(parsedWorkflow.jobs),
    "release workflow has no jobs",
  );

  const jobBodies = new Map(
    Object.entries(parsedWorkflow.jobs),
  );
  assert(
    jobBodies.size > 0,
    "release workflow has no jobs",
  );
  for (const [job, definition] of jobBodies) {
    assert.match(
      job,
      JOB_ID,
      `release workflow has invalid job ID ${job}`,
    );
    assert(
      isRecord(definition),
      `${job} job is not a mapping`,
    );
  }

  return { jobBodies, parsedWorkflow };
};

export const effectiveOidcJobNames = ({
  jobBodies,
  workflowPermissions,
}) =>
  [...jobBodies]
    .filter(([, job]) => {
      const permissions =
        job.permissions ?? workflowPermissions;
      return (
        permissions === "write-all" ||
        (isRecord(permissions) &&
          permissions["id-token"] === "write")
      );
    })
    .map(([job]) => job)
    .sort((left, right) => left.localeCompare(right));

export const effectiveWriteGrants = ({
  jobBodies,
  workflowPermissions,
}) =>
  [...jobBodies]
    .flatMap(([jobName, job]) => {
      const permissions =
        job.permissions ?? workflowPermissions;
      if (permissions === "write-all") {
        return [`${jobName}:write-all`];
      }
      assert(
        isRecord(permissions),
        `${jobName} permissions are not a mapping`,
      );
      return Object.entries(permissions)
        .filter(([, access]) => access === "write")
        .map(([permission]) => `${jobName}:${permission}`);
    })
    .sort((left, right) => left.localeCompare(right));

export const parseStepRuns = (job) => {
  const steps = job.steps ?? [];
  assert(
    Array.isArray(steps),
    "job steps are not a sequence",
  );
  const runs = [];
  for (const step of steps) {
    assert(
      isRecord(step),
      "workflow step is not a mapping",
    );
    if (step.run === undefined) continue;
    assert.equal(
      typeof step.run,
      "string",
      "step run is not a string",
    );
    assert(
      step.name === undefined ||
        typeof step.name === "string",
      "step name is not a string",
    );
    runs.push({ name: step.name, run: step.run });
  }
  return runs;
};

const actionReferences = (job) => {
  const references = [];
  if (job.uses !== undefined) {
    assert.equal(typeof job.uses, "string");
    references.push(job.uses);
  }
  const steps = job.steps ?? [];
  assert(
    Array.isArray(steps),
    "job steps are not a sequence",
  );
  for (const step of steps) {
    assert(
      isRecord(step),
      "workflow step is not a mapping",
    );
    if (step.uses === undefined) continue;
    assert.equal(typeof step.uses, "string");
    references.push(step.uses);
  }
  return references;
};

export const checkReleasePrivilegeBoundary = (workflow) => {
  const { jobBodies, parsedWorkflow } =
    parseJobBodies(workflow);
  assert.deepEqual(
    parsedWorkflow.permissions,
    { contents: "read" },
    "release workflow permission ceiling changed",
  );

  const body = (job) => {
    const definition = jobBodies.get(job);
    assert(
      definition,
      `release workflow is missing the ${job} job`,
    );
    return definition;
  };

  const oidcJobs = effectiveOidcJobNames({
    jobBodies,
    workflowPermissions: parsedWorkflow.permissions,
  });
  assert.deepEqual(
    oidcJobs,
    [
      "attest",
      "core-recovery-attest",
      "finalize",
      "publish-core",
    ],
    "release OIDC job allowlist changed",
  );
  assert.deepEqual(
    effectiveWriteGrants({
      jobBodies,
      workflowPermissions: parsedWorkflow.permissions,
    }),
    [
      "attest:attestations",
      "attest:id-token",
      "core-recovery-attest:attestations",
      "core-recovery-attest:id-token",
      "finalize:contents",
      "finalize:id-token",
      "publish-core:id-token",
    ],
    "release write-permission allowlist changed",
  );

  const privilegedPermissions = {
    attest: {
      contents: "read",
      attestations: "write",
      "id-token": "write",
    },
    "core-recovery-attest": {
      contents: "read",
      attestations: "write",
      "id-token": "write",
    },
    finalize: {
      contents: "write",
      "id-token": "write",
    },
    "publish-core": {
      contents: "read",
      "id-token": "write",
    },
  };
  for (const job of oidcJobs) {
    assert.deepEqual(
      body(job).permissions,
      privilegedPermissions[job],
      `${job} privileged permissions changed`,
    );
  }

  const privilegedActionAllowlist = {
    attest: [
      "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
      "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
      "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
      "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
    ],
    "core-recovery-attest": [
      "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
      "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
    ],
    finalize: [
      "stella/.github/.github/workflows/npm-version-finalize.yml@1ce0079bbdbf93a4c1917d2857496b89aedcec14",
    ],
    "publish-core": [
      "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
      "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
      "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
      "rust-lang/crates-io-auth-action@c6f97d42243bad5fab37ca0427f495c86d5b1a18",
    ],
  };
  for (const job of oidcJobs) {
    assert.deepEqual(
      actionReferences(body(job)),
      privilegedActionAllowlist[job],
      `${job} privileged action allowlist changed`,
    );
  }

  const privilegedRunAllowlist = {
    attest: [],
    "core-recovery-attest": [],
    finalize: [],
    "publish-core": [
      {
        name: "Validate core crate artifact",
        sha256:
          "6da47bcafc950a6fd71dbbd95165b4e4dfa6aecd9a57122afedffe98f81bbf9c",
      },
      {
        name: "Recheck crates.io version",
        sha256:
          "cf147deabaf7744379e217605821175033ed61836955ee2f7cbf5c0bf8525382",
      },
      {
        name: "Publish core crate",
        sha256:
          "136b6b2afad1ccd3a65f8f46471b3e6359ecab9736b277e2115384ff9cc55b5a",
      },
      {
        name: "Verify published crate",
        sha256:
          "f233c2ef8476cb0881cdd73239510faec0bcb4066d18c096af71ecefd1171964",
      },
    ],
  };
  for (const job of oidcJobs) {
    const fingerprints = parseStepRuns(body(job)).map(
      stepRunFingerprint,
    );
    assert.deepEqual(
      fingerprints,
      privilegedRunAllowlist[job],
      `${job} privileged run-step allowlist changed`,
    );
  }

  assert.deepEqual(
    body("finalize").secrets,
    {
      RELEASE_APP_ID: "${{ secrets.RELEASE_APP_ID }}",
      RELEASE_APP_PRIVATE_KEY:
        "${{ secrets.RELEASE_APP_PRIVATE_KEY }}",
      CHANGELOG_APP_ID: "${{ secrets.CHANGELOG_APP_ID }}",
      CHANGELOG_APP_PRIVATE_KEY:
        "${{ secrets.CHANGELOG_APP_PRIVATE_KEY }}",
    },
    "finalize secret allowlist changed",
  );

  const privilegedJobFingerprints = {
    attest:
      "19da37d95ae200a4063c69de5c95938d74f69f24d3c3be410909e0ad46d1c227",
    "core-recovery-attest":
      "162df979eb880981b8d6bbf215c27e2934cb0d838097edd0e98aeeca5f87b328",
    finalize:
      "be2621a97aafb93e5a62f66688d47db9fa186c83444a9fdd3c30b1fd09a50ae8",
    "publish-core":
      "cdc631bfa8c9cff8737d42be6be833f6cba29af7744ed9e34f40b4fb3d180c8a",
  };
  for (const job of oidcJobs) {
    assert.equal(
      fingerprint(body(job)),
      privilegedJobFingerprints[job],
      `${job} complete privileged job contract changed`,
    );
  }
};

const isMain =
  process.argv[1] !== undefined &&
  pathToFileURL(resolve(process.argv[1])).href ===
    import.meta.url;
if (isMain) {
  const workflow = await readFile(
    new URL(
      "../.github/workflows/release.yml",
      import.meta.url,
    ),
    "utf8",
  );
  checkReleasePrivilegeBoundary(workflow);
}
