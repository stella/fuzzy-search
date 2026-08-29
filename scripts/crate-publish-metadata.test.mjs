import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";

const metadata = JSON.parse(
  execFileSync(
    process.execPath,
    ["scripts/crate-publish-metadata.mjs"],
    {
      encoding: "utf8",
    },
  ),
);

void test("crate upload metadata matches the synchronized release", () => {
  assert.equal(metadata.name, "stella-fuzzy-search-core");
  assert.equal(
    metadata.vers,
    readFileSync("VERSION", "utf8").trim(),
  );
  assert.equal(metadata.readme_file, "README.md");
  assert.equal(
    metadata.readme,
    readFileSync("crates/core/README.md", "utf8"),
  );
});

void test("every dependency has the crates.io publish fields", () => {
  assert.ok(metadata.deps.length > 0);
  for (const dependency of metadata.deps) {
    assert.equal(typeof dependency.name, "string");
    assert.match(dependency.version_req, /^\^?[0-9]/);
    assert.ok(Array.isArray(dependency.features));
    assert.equal(typeof dependency.optional, "boolean");
    assert.equal(
      typeof dependency.default_features,
      "boolean",
    );
    assert.ok(
      ["normal", "build", "dev"].includes(dependency.kind),
    );
  }
});
