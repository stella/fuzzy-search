import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

import {
  checkReleasePrivilegeBoundary,
  effectiveOidcJobNames,
  parseJobBodies,
} from "./check-release-privilege-boundary.mjs";

const workflow = readFileSync(
  new URL(
    "../.github/workflows/release.yml",
    import.meta.url,
  ),
  "utf8",
);

const mutate = (source, expected, replacement) => {
  const mutation = source.replace(expected, replacement);
  expect(mutation).not.toBe(source);
  return mutation;
};

describe("release privilege boundary", () => {
  test("recognizes the complete GitHub job identifier grammar", () => {
    const fixture = `
jobs:
  a:
    runs-on: ubuntu-latest
  _:
    runs-on: ubuntu-latest
  publish_npm:
    runs-on: ubuntu-latest
  Publish-1:
    runs-on: ubuntu-latest
  'quoted_id':
    runs-on: ubuntu-latest
  "Q":
    runs-on: ubuntu-latest
`;

    expect([
      ...parseJobBodies(fixture).jobBodies.keys(),
    ]).toEqual([
      "a",
      "_",
      "publish_npm",
      "Publish-1",
      "quoted_id",
      "Q",
    ]);
  });

  test("computes OIDC after workflow permissions are inherited", () => {
    const { jobBodies, parsedWorkflow } = parseJobBodies(`
permissions:
  id-token: write
jobs:
  inherited:
    runs-on: ubuntu-latest
  narrowed:
    runs-on: ubuntu-latest
    permissions:
      contents: read
  write_all:
    runs-on: ubuntu-latest
    permissions: write-all
`);

    expect(
      effectiveOidcJobNames({
        jobBodies,
        workflowPermissions: parsedWorkflow.permissions,
      }),
    ).toEqual(["inherited", "write_all"]);
  });

  test("rejects workflow-level OIDC and write-all", () => {
    const permissionCeiling = `permissions:
  contents: read`;
    for (const replacement of [
      `${permissionCeiling}
  id-token: write`,
      "permissions: write-all",
    ]) {
      const mutation = mutate(
        workflow,
        permissionCeiling,
        replacement,
      );
      expect(() =>
        checkReleasePrivilegeBoundary(mutation),
      ).toThrow(
        "release workflow permission ceiling changed",
      );
    }
  });

  test("rejects arbitrary privileged actions and shell commands", () => {
    const finalizeMarker = "\n  finalize:";
    const mutations = [
      `
      - name: Unreviewed privileged command
        run: bash scripts/arbitrary.sh
`,
      `
      - name: Unreviewed privileged command
        run: make
`,
      `
      - name: Unreviewed privileged action
        uses: example/action@0123456789012345678901234567890123456789
`,
    ];

    for (const addition of mutations) {
      const mutation = mutate(
        workflow,
        finalizeMarker,
        `${addition}${finalizeMarker}`,
      );
      expect(() =>
        checkReleasePrivilegeBoundary(mutation),
      ).toThrow(
        /publish-core privileged (?:run-step|action) allowlist changed/,
      );
    }
  });

  test("rejects privileged action inputs, execution fields, and every write grant", () => {
    const mutations = [
      mutate(
        workflow,
        "          name: release-artifacts\n          path: release-artifacts\n      - name: Download recovery artifact",
        "          name: release-artifacts\n          repository: attacker/repository\n          path: release-artifacts\n      - name: Download recovery artifact",
      ),
      mutate(
        workflow,
        "      - name: Validate core crate artifact\n        id: crate",
        "      - name: Validate core crate artifact\n        shell: bash -c 'echo unreviewed; {0}'\n        id: crate",
      ),
      mutate(
        workflow,
        "  pack:\n    name: Pack\n    needs: [preflight, verify, test]\n    if: needs.preflight.outputs.already-released != 'true'\n    runs-on: ubuntu-latest\n    permissions:\n      contents: read",
        "  pack:\n    name: Pack\n    needs: [preflight, verify, test]\n    if: needs.preflight.outputs.already-released != 'true'\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write",
      ),
    ];

    expect(() =>
      checkReleasePrivilegeBoundary(mutations[0]),
    ).toThrow("complete privileged job contract changed");
    expect(() =>
      checkReleasePrivilegeBoundary(mutations[1]),
    ).toThrow("complete privileged job contract changed");
    expect(() =>
      checkReleasePrivilegeBoundary(mutations[2]),
    ).toThrow("release write-permission allowlist changed");
  });

  test("binds the finalizer workflow and exact secret surface", () => {
    const pinMutation = mutate(
      workflow,
      "npm-version-finalize.yml@1ce0079bbdbf93a4c1917d2857496b89aedcec14",
      "npm-version-finalize.yml@0000000000000000000000000000000000000000",
    );
    expect(() =>
      checkReleasePrivilegeBoundary(pinMutation),
    ).toThrow(
      "finalize privileged action allowlist changed",
    );

    const secretMutation = mutate(
      workflow,
      "      CHANGELOG_APP_PRIVATE_KEY: ${{ secrets.CHANGELOG_APP_PRIVATE_KEY }}",
      [
        "      CHANGELOG_APP_PRIVATE_KEY: ${{ secrets.CHANGELOG_APP_PRIVATE_KEY }}",
        "      EXTRA_SECRET: ${{ secrets.EXTRA_SECRET }}",
      ].join("\n"),
    );
    expect(() =>
      checkReleasePrivilegeBoundary(secretMutation),
    ).toThrow("finalize secret allowlist changed");
  });

  test("crate publication never retries PUT and proves the exact published bytes", () => {
    expect(
      workflow.match(
        /https:\/\/crates\.io\/api\/v1\/crates\/new/g,
      ),
    ).toHaveLength(1);
    expect(workflow).toMatch(
      /upload_status="\$\(curl[^\n]*--max-time 600[^\n]*\\/,
    );
    expect(workflow).not.toMatch(
      /upload_status="\$\(curl[^\n]*--retry/,
    );
    const publishCore =
      parseJobBodies(workflow).jobBodies.get(
        "publish-core",
      );
    expect(publishCore).toBeDefined();
    expect(publishCore.if).not.toContain(
      "needs.core-preflight.outputs.already-released != 'true'",
    );
    expect(workflow).toContain("CRATE_CHECKSUM");
    expect(workflow).toContain("sha256sum");
    expect(workflow).toContain(
      "ambiguous upload committed the exact crate bytes",
    );
  });

  test("accepts the reviewed workflow as a fixed point", () => {
    checkReleasePrivilegeBoundary(workflow);
    expect(
      parseJobBodies(workflow).jobBodies.size,
    ).toBeGreaterThan(0);
  });
});
