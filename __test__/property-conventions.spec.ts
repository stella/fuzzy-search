import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

import seeds from "./property-seeds.json";

const root = import.meta.dir;
const entry = path.join(root, "properties.spec.ts");
const helper = path.join(root, "property-testing.ts");
const transpiler = new Bun.Transpiler({ loader: "ts" });

const filesUnder = (directory: string): string[] => {
  const files: string[] = [];
  for (const item of readdirSync(directory, {
    withFileTypes: true,
  })) {
    const file = path.join(directory, item.name);
    if (item.isDirectory()) {
      for (const child of filesUnder(file))
        files.push(child);
    } else if (item.isFile() && item.name.endsWith(".ts"))
      files.push(file);
  }
  return files;
};

const files = filesUnder(root).map((file) => ({
  file,
  relative: path.relative(root, file),
  source: readFileSync(file, "utf8"),
}));

// Preserve offsets so a detected call can be read from the original source.
const codeOnly = (source: string): string =>
  source
    .replace(
      /'(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|`(?:\\.|[^`\\])*`/gs,
      (s) => s.replace(/[^\r\n]/g, " "),
    )
    .replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, (s) =>
      s.replace(/[^\r\n]/g, " "),
    );

const importsFrom = (source: string, module: string) => {
  const code = codeOnly(source);
  return [
    ...source.matchAll(
      /\bimport\s+([^;]+?)\s+from\s*["']([^"']+)["']\s*;/gu,
    ),
  ]
    .filter(
      (match) =>
        match[2] === module &&
        match.index !== undefined &&
        /^\s*import\b/u.test(code.slice(match.index)),
    )
    .map((match) => match[1] ?? "");
};

const localsFor = (
  clause: string,
  names: readonly string[],
) => {
  const locals: { name: string; imported: string }[] = [];
  const defaultName = clause.match(/^\s*([\w$]+)/u)?.[1];
  if (defaultName !== undefined && defaultName !== "type") {
    locals.push({ name: defaultName, imported: "default" });
  }
  const namespace = clause.match(
    /\*\s+as\s+([\w$]+)/u,
  )?.[1];
  if (namespace !== undefined)
    locals.push({ name: namespace, imported: "namespace" });
  const named = clause.match(/\{([^}]+)\}/u)?.[1];
  for (const part of named?.split(",") ?? []) {
    const [imported, alias] = part
      .trim()
      .split(/\s+as\s+/u);
    if (imported === undefined) continue;
    const name = alias?.trim() ?? imported.trim();
    if (names.includes(imported.trim()))
      locals.push({ name, imported: imported.trim() });
  }
  return locals;
};

const callsFor = (
  source: string,
  module: string,
  names: readonly string[],
) => {
  const code = codeOnly(source);
  const calls: string[] = [];
  for (const clause of importsFrom(source, module)) {
    for (const local of localsFor(clause, names)) {
      for (const name of names) {
        if (
          local.imported !== "default" &&
          local.imported !== "namespace" &&
          local.imported !== name
        )
          continue;
        const callee =
          local.imported === "default"
            ? `${local.name}\\s*\\.\\s*${name}`
            : local.imported === "namespace"
              ? `${local.name}\\s*\\.\\s*${name}`
              : local.name;
        if (
          new RegExp(`\\b${callee}\\s*\\(`, "u").test(code)
        ) {
          calls.push(
            local.imported === "namespace" ||
              local.imported === "default"
              ? `${local.name}.${name}`
              : local.name,
          );
        }
      }
    }
  }
  return calls;
};

const helperBindings = (source: string) => {
  const clauses = importsFrom(source, "./property-testing");
  return clauses.flatMap((clause) =>
    localsFor(clause, ["assertProperty"]).map(
      ({ name, imported }) =>
        imported === "namespace"
          ? `${name}.assertProperty`
          : name,
    ),
  );
};

const helperIds = (source: string) => {
  const code = codeOnly(source);
  const ids: string[] = [];
  for (const binding of helperBindings(source)) {
    const pattern = new RegExp(
      `\\b${binding.replaceAll(".", "\\s*\\.\\s*")}\\s*\\(`,
      "gu",
    );
    for (const match of code.matchAll(pattern)) {
      const start = match.index;
      if (start === undefined) continue;
      const literal = source
        .slice(start)
        .match(
          /^\s*[\w$.]+\s*\(\s*(["'])([^"'\\]+)\1\s*,/u,
        )?.[2];
      if (literal !== undefined) ids.push(literal);
    }
  }
  return ids;
};

const helperCallCount = (source: string) =>
  helperBindings(source).reduce((count, binding) => {
    const pattern = new RegExp(
      `\\b${binding.replaceAll(".", "\\s*\\.\\s*")}\\s*\\(`,
      "gu",
    );
    return (
      count + [...codeOnly(source).matchAll(pattern)].length
    );
  }, 0);

const resolveImport = (from: string, specifier: string) => {
  const base = path.resolve(path.dirname(from), specifier);
  const candidates = [
    base,
    `${base}.ts`,
    `${base}.tsx`,
    path.join(base, "index.ts"),
  ];
  return candidates.find((candidate) =>
    files.some(({ file }) => file === candidate),
  );
};

const reachableFromEntry = () => {
  const visited = new Set<string>();
  const pending = [entry];
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === undefined || visited.has(current))
      continue;
    visited.add(current);
    const source = files.find(
      ({ file }) => file === current,
    )?.source;
    if (source === undefined) continue;
    for (const { path: imported } of transpiler.scanImports(
      source,
    )) {
      if (!imported.startsWith(".")) continue;
      const resolved = resolveImport(current, imported);
      if (resolved !== undefined) pending.push(resolved);
    }
  }
  return new Set(
    [...visited].map((file) => path.relative(root, file)),
  );
};

const assertionFiles = files
  .filter(({ file }) => file !== helper)
  .flatMap(({ relative, source }) =>
    helperIds(source).map((id) => ({ id, relative })),
  );
const ids = assertionFiles.map(({ id }) => id);
const duplicateIds = ids.filter(
  (id, index) => ids.indexOf(id) !== index,
);
const violations = files.flatMap(
  ({ file, relative, source }) => {
    if (file === helper) return [];
    const problems: string[] = [];
    if (
      callsFor(source, "fast-check", ["assert", "check"])
        .length > 0
    )
      problems.push(
        `${relative}: raw fast-check assertion`,
      );
    if (
      callsFor(source, "fast-check", ["property"]).length >
        0 &&
      helperIds(source).length === 0
    )
      problems.push(
        `${relative}: property must use assertProperty from ./property-testing`,
      );
    if (
      helperCallCount(source) !== helperIds(source).length
    )
      problems.push(
        `${relative}: assertProperty needs a literal id`,
      );
    return problems;
  },
);

const seedProblems = Object.entries(seeds).flatMap(
  ([id, entries]) => {
    const problems: string[] = [];
    if (
      ids.filter((propertyId) => propertyId === id)
        .length !== 1
    )
      problems.push(
        `${id}: expected one literal assertProperty id`,
      );
    for (const [index, seed] of entries.entries()) {
      if (
        !Number.isInteger(seed.seed) ||
        seed.seed < -2_147_483_648 ||
        seed.seed > 2_147_483_647
      )
        problems.push(
          `${id}[${index}]: seed must be signed int32`,
        );
      if (!/^\d+(?::\d+)*$/u.test(seed.path))
        problems.push(
          `${id}[${index}]: invalid replay path`,
        );
      if (seed.note.trim().length === 0)
        problems.push(
          `${id}[${index}]: note must be nonempty`,
        );
      const date = new Date(seed.date);
      if (
        !/^\d{4}-\d{2}-\d{2}$/u.test(seed.date) ||
        !Number.isFinite(date.getTime()) ||
        date.toISOString().slice(0, 10) !== seed.date
      )
        problems.push(
          `${id}[${index}]: date must be valid YYYY-MM-DD`,
        );
    }
    return problems;
  },
);

describe("property test conventions", () => {
  test("properties use assertProperty and avoid raw fast-check calls", () => {
    expect(violations).toEqual([]);
  });
  test("property ids are literal and unique", () => {
    expect(duplicateIds).toEqual([]);
    expect(assertionFiles.length).toBeGreaterThan(0);
  });
  test("pinned seed metadata is valid and resolves to one property", () => {
    expect(seedProblems).toEqual([]);
  });
  test("property files are reachable from properties.spec.ts", () => {
    const reachable = reachableFromEntry();
    const asserted = new Set(
      assertionFiles.map(({ relative }) => relative),
    );
    expect(
      [...asserted].filter((file) => !reachable.has(file)),
    ).toEqual([]);
  });
  test("source samples and comments do not count as calls", () => {
    const sample = `import { test } from "bun:test";\nimport fc from "fast-check";\nconst example = "fc.assert(value)";\n\`fc.check(value)\`;\n// fc.assert(value)\nconst pattern = /fc\\.assert\\(/u;`;
    expect(
      callsFor(sample, "fast-check", ["assert", "check"]),
    ).toEqual([]);
  });
  test("imports after bun:test retain their aliases and helper ids", () => {
    const sample = `import { test } from "bun:test";\nimport checks from "fast-check";\nimport { assertProperty as verify } from "./property-testing";\ntest("fixture", () => { checks.assert(value); verify("sample-id", property); });`;
    expect(
      callsFor(sample, "fast-check", ["assert", "check"]),
    ).toEqual(["checks.assert"]);
    expect(helperIds(sample)).toEqual(["sample-id"]);
  });
  test("Unicode source preserves the helper call offsets", () => {
    const sample = `import { assertProperty } from "./property-testing";\nconst text = "😀";\nassertProperty("unicode-id", property);`;
    expect(codeOnly(sample).length).toBe(sample.length);
    expect(helperIds(sample)).toEqual(["unicode-id"]);
  });
});
