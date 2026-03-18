/**
 * Speed benchmark for @stll/fuzzy-search.
 *
 * Compares against JS ecosystem alternatives:
 * - fastest-levenshtein + sliding window
 * - naive JS Levenshtein + sliding window
 * - fuse.js (word-split approach)
 * - fuzzball (Python rapidfuzz port)
 *
 * Run: bun run bench:speed
 * Install deps first: bun run bench:install
 */

import {
  bench,
  CZECH_NAMES,
  ENGLISH_NAMES,
  libs,
  printSpeedups,
} from "./helpers";

// ─── Corpus generation ───────────────────────

function generateLegalText(
  sizeKB: number,
): string {
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

function generateEnglishText(
  sizeKB: number,
): string {
  const words = [
    "agreement", "between", "party", "first",
    "second", "hereinafter", "referred", "shall",
    "pursuant", "section", "whereas", "covenant",
    "liability", "breach", "warranty", "damages",
    "termination", "binding", "jurisdiction",
    "arbitration", "indemnify", "executed",
    "consideration", "amendment", "provision",
    "obligation", "representation", "dispute",
  ];
  const parts: string[] = [];
  let size = 0;
  const target = sizeKB * 1024;
  let i = 0;
  while (size < target) {
    if (i % 200 === 50) {
      parts.push("Jonhson");
    } else if (i % 200 === 100) {
      parts.push("Willaims");
    } else if (i % 200 === 150) {
      parts.push("Tompson");
    } else {
      parts.push(words[i % words.length]!);
    }
    size += parts.at(-1)!.length + 1;
    i++;
  }
  return parts.join(" ");
}

// ─── Benchmarks ──────────────────────────────

const N = 5;

console.log("=".repeat(62));
console.log(" FUZZY SEARCH BENCHMARKS");
console.log(
  " @stll/fuzzy-search vs JS ecosystem",
);
console.log("=".repeat(62));

const czech64 = generateLegalText(64);
const english64 = generateEnglishText(64);
const czech256 = generateLegalText(256);

const scenarios = [
  {
    label: `Czech legal (${(czech64.length / 1024).toFixed(0)}KB) × 5 names, dist 1-2`,
    patterns: CZECH_NAMES,
    haystack: czech64,
  },
  {
    label: `English legal (${(english64.length / 1024).toFixed(0)}KB) × 5 names, dist 1-2`,
    patterns: ENGLISH_NAMES,
    haystack: english64,
  },
  {
    label: `Czech legal (${(czech256.length / 1024).toFixed(0)}KB) × 5 names, dist 1-2`,
    patterns: CZECH_NAMES,
    haystack: czech256,
    skipSlow: true,
  },
  {
    label: `Czech legal (${(czech64.length / 1024).toFixed(0)}KB) × 1 name, dist 1`,
    patterns: [
      { pattern: "Gaislerová", distance: 1 },
    ],
    haystack: czech64,
  },
];

for (const s of scenarios) {
  console.log(`\n### ${s.label}\n`);
  const times: number[] = [];
  for (let i = 0; i < libs.length; i++) {
    const lib = libs[i]!;
    // Skip slow libraries on large corpora
    if (
      s.skipSlow &&
      (lib.name.includes("naive") ||
        lib.name.includes("fuse") ||
        lib.name.includes("fuzzball"))
    ) {
      times.push(Number.NaN);
      continue;
    }
    const engine = lib.build(s.patterns);
    times.push(
      bench(
        lib.name,
        () => lib.search(engine, s.haystack),
        lib.name.includes("naive") ||
          lib.name.includes("fuzzball")
          ? 3
          : N,
      ),
    );
  }
  printSpeedups(
    times.filter((t) => !Number.isNaN(t)),
  );
}

console.log("\n" + "=".repeat(62));
console.log(" Done.");
console.log("=".repeat(62));
