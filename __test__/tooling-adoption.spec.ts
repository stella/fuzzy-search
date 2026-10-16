import { library } from "@stll/oxlint-config";
import {
  portableSafetyPluginSpecifiers,
  portableSafetyRules,
} from "@stll/oxlint-plugin";
import { test, expect } from "bun:test";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

import config from "../oxlint.config";

const read = (path: string) =>
  readFileSync(
    new URL(`../${path}`, import.meta.url),
    "utf8",
  );

test("portable safety rules apply to source and every shared override", () => {
  expect(config.jsPlugins).toEqual(
    expect.arrayContaining(portableSafetyPluginSpecifiers),
  );
  expect(config.rules).toMatchObject(portableSafetyRules);
  expect(config.overrides).toHaveLength(
    library().overrides?.length ?? 0,
  );
  for (const override of config.overrides ?? []) {
    expect(override.rules).toMatchObject(
      portableSafetyRules,
    );
  }
  expect(config.options).toEqual(library().options);
});

test("the shared detector fails closed for unavailable history", () => {
  const result = JSON.parse(
    execFileSync(
      "bunx",
      [
        "--no-install",
        "stll-toolchain-changed",
        "--since",
        "missing-toolchain-history",
      ],
      { encoding: "utf8" },
    ),
  );
  expect(result).toMatchObject({
    changed: true,
    status: "unreadable",
  });
});

test("generated cloud setup is a fixed point and preserves the startup hook", () => {
  const paths = [
    ".agents/cloud-setup.sh",
    ".claude/settings.json",
  ];
  const before = paths.map(read);
  execFileSync("bunx", [
    "--no-install",
    "stll-cloud-setup",
  ]);
  expect(paths.map(read)).toEqual(before);
  expect(before.at(0)).toContain("--frozen-lockfile");
  expect(before.at(1)).toContain("CLAUDE_CODE_REMOTE");
  expect(before.at(1)).toContain(".agents/cloud-setup.sh");
});
