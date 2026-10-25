import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const read = (path: string) =>
  readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

test("CI type checks with bun check in the repository job", () => {
  const workflow = read(".github/workflows/ci.yml");
  expect(workflow).toContain("__test__/tooling-adoption.spec.ts");
  expect(workflow).toContain("      - run: bun run typecheck");
  expect(workflow).toContain("export const typecheckRejectionProbe: string");
  // The shared workflow's type check job also runs compiler parity.
  expect(workflow).not.toContain("      typecheck: true");
  expect(read("package.json")).toContain(
    '"typecheck": "bun check --no-pretty --all --project=tsconfig.json"',
  );
});
