/**
 * Speed benchmark for @stll/fuzzy-search.
 *
 * Tests fuzzy matching performance on legal
 * document text. Compares against naive JS
 * implementation.
 *
 * Run: bun run bench:speed
 */

import { FuzzySearch } from "../lib";

// ─── Helpers ─────────────────────────────────

function bench(
  name: string,
  fn: () => void,
  n: number,
): number {
  // Warmup
  fn();
  fn();

  const times: number[] = [];
  for (let i = 0; i < n; i++) {
    const start = performance.now();
    fn();
    times.push(performance.now() - start);
  }
  times.sort((a, b) => a - b);
  const median = times[Math.floor(n / 2)]!;
  console.log(
    `  ${name.padEnd(30)} ${median.toFixed(3)} ms (median of ${n})`,
  );
  return median;
}

// ─── Naive JS oracle for comparison ──────────

function naiveLevenshtein(
  a: string,
  b: string,
): number {
  const m = a.length;
  const n = b.length;
  const prev = Array.from(
    { length: n + 1 },
    (_, i) => i,
  );
  for (let i = 1; i <= m; i++) {
    const curr = [i];
    for (let j = 1; j <= n; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      curr[j] = Math.min(
        curr[j - 1]! + 1,
        prev[j]! + 1,
        prev[j - 1]! + cost,
      );
    }
    prev.splice(0, prev.length, ...curr);
  }
  return prev[n]!;
}

function naiveFuzzySearch(
  patterns: { pattern: string; distance: number }[],
  text: string,
): number {
  let count = 0;
  for (const { pattern, distance } of patterns) {
    const m = pattern.length;
    for (let i = 0; i <= text.length - m; i++) {
      for (
        let len = Math.max(1, m - distance);
        len <= m + distance && i + len <= text.length;
        len++
      ) {
        const window = text.slice(i, i + len);
        if (naiveLevenshtein(pattern, window) <= distance) {
          count++;
          break;
        }
      }
    }
  }
  return count;
}

// ─── Corpus ──────────────────────────────────

const CZECH_NAMES = [
  { pattern: "Gaislerová", distance: 1 },
  { pattern: "Novák", distance: 1 },
  { pattern: "Šnytrová", distance: 1 },
  { pattern: "Příbram", distance: 2 },
  { pattern: "Dvořák", distance: 1 },
];

// Generate synthetic legal text (~64KB)
function generateLegalText(sizeKB: number): string {
  const words = [
    "smlouva", "podepsal", "nájemní", "byt",
    "město", "okres", "pan", "paní", "dne",
    "roku", "příloha", "dodatek", "částka",
    "korun", "českých", "smluvní", "strana",
    "pronajímatel", "nájemce", "předmět",
    "nájmu", "doba", "určitá", "neurčitá",
    "výpovědní", "lhůta", "měsíce", "zákon",
    "občanský", "zákoník", "ustanovení",
  ];
  const parts: string[] = [];
  let size = 0;
  const target = sizeKB * 1024;
  let i = 0;
  while (size < target) {
    // Sprinkle in some fuzzy variants of names
    if (i % 200 === 50) {
      parts.push("Gais1erová");
    } else if (i % 200 === 100) {
      parts.push("Nowák");
    } else if (i % 200 === 150) {
      parts.push("Pribram");
    } else {
      parts.push(words[i % words.length]!);
    }
    size += parts.at(-1)!.length + 1;
    i++;
  }
  return parts.join(" ");
}

// ─── Benchmarks ──────────────────────────────

const N = 10;

console.log("=".repeat(56));
console.log(" FUZZY SEARCH BENCHMARKS");
console.log("=".repeat(56));

const text64 = generateLegalText(64);
const text256 = generateLegalText(256);

const scenarios = [
  {
    label: `5 names, dist 1-2, 64KB`,
    patterns: CZECH_NAMES,
    text: text64,
  },
  {
    label: `5 names, dist 1-2, 256KB`,
    patterns: CZECH_NAMES,
    text: text256,
  },
  {
    label: `1 name, dist 1, 64KB`,
    patterns: [
      { pattern: "Gaislerová", distance: 1 },
    ],
    text: text64,
  },
];

for (const s of scenarios) {
  console.log(`\n### ${s.label}\n`);

  const fs = new FuzzySearch(
    s.patterns.map((p) => ({
      pattern: p.pattern,
      distance: p.distance,
    })),
    { wholeWords: true },
  );

  const nativeTime = bench(
    "@stll/fuzzy-search",
    () => fs.findIter(s.text),
    N,
  );

  // Only run naive on small corpus (it's slow)
  if (s.text.length <= 70000) {
    const naiveTime = bench(
      "naive JS (sliding window)",
      () => naiveFuzzySearch(s.patterns, s.text),
      3,
    );
    console.log(
      `  → Speedup: ${(naiveTime / nativeTime).toFixed(1)}x`,
    );
  }
}

console.log("\n" + "=".repeat(56));
console.log(" Done.");
console.log("=".repeat(56));
