#!/usr/bin/env node
/**
 * Post-build check: scan the WASM browser bundle for Node.js-only APIs
 * that would crash at runtime in a browser environment.
 *
 * Run after `build:js` to verify the output is browser-safe:
 *   node scripts/check-wasm-compat.mjs
 */
import { readFileSync } from "node:fs";

const WASM_DIST = "wasm/dist/wasm.mjs";

let code;
try {
  code = readFileSync(WASM_DIST, "utf8");
} catch {
  console.error(`\u2717 ${WASM_DIST} not found \u2014 run build:js first`);
  process.exit(1);
}

// Match value-position patterns (API calls, property access) rather than
// trying to strip strings/comments — bundled output makes stripping fragile.
/** @type {{ pattern: RegExp; name: string }[]} */
const BANNED = [
  // Any Buffer member access or constructor
  { pattern: /\bBuffer\s*\.\s*\w+\s*\(/g, name: "Buffer.*()" },
  { pattern: /new\s+Buffer\s*\(/g, name: "new Buffer()" },
  // CJS require
  { pattern: /\brequire\s*\(\s*["']/g, name: "require()" },
  // Node process globals
  {
    pattern: /\bprocess\s*\.\s*(env|argv|cwd|exit|stdout|stderr|pid|platform|arch)\b/g,
    name: "process.*",
  },
  // Node CJS globals
  { pattern: /\b__dirname\b/g, name: "__dirname" },
  { pattern: /\b__filename\b/g, name: "__filename" },
  // Node built-in module imports (static and dynamic)
  {
    pattern: /(?:from|import)\s*["']node:/g,
    name: 'import "node:*"',
  },
  {
    pattern: /import\s*\(\s*["']node:/g,
    name: 'import("node:*")',
  },
];

/** @type {{ name: string; line: number; snippet: string }[]} */
const issues = [];

for (const { pattern, name } of BANNED) {
  for (const m of code.matchAll(pattern)) {
    const line = code.slice(0, m.index).split("\n").length;
    const lineContent = code.split("\n")[line - 1].trim();
    issues.push({ name, line, snippet: lineContent.slice(0, 80) });
  }
}

if (issues.length > 0) {
  console.error(`\u2717 ${WASM_DIST} contains Node.js-only APIs:\n`);
  for (const { name, line, snippet } of issues) {
    console.error(`  line ${line}: ${name}`);
    console.error(`    ${snippet}`);
  }
  console.error(
    `\nThese will crash in the browser. Fix the source or add a browser-compatible alternative.`,
  );
  process.exit(1);
}

console.log(
  `\u2713 ${WASM_DIST} is browser-compatible (no Node.js globals found)`,
);
