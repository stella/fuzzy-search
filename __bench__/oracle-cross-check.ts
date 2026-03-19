/**
 * Cross-check: verify our distance computations
 * against independent npm implementations.
 *
 * For every match our library produces, compute
 * the distance using damerau-levenshtein (npm)
 * and js-levenshtein (npm) and verify they agree.
 *
 * This is the definitive correctness oracle: our
 * values must match battle-tested third-party
 * implementations.
 *
 * Run: bun __bench__/oracle-cross-check.ts
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";

// @ts-expect-error — no type declarations
import damNpm from "damerau-levenshtein";
// @ts-expect-error — no type declarations
import jsLev from "js-levenshtein";

import { FuzzySearch } from "../lib";

// ─── Reference distance functions ────────────

function refLevenshtein(
  a: string,
  b: string,
): number {
  return jsLev(a, b) as number;
}

function refDamerau(
  a: string,
  b: string,
): number {
  const r = damNpm(a, b) as { steps: number };
  return r.steps;
}

// ─── Cross-check runner ──────────────────────

type Result = {
  label: string;
  total: number;
  levOk: number;
  damOk: number;
  levFail: {
    pattern: string;
    text: string;
    ours: number;
    theirs: number;
  }[];
  damFail: {
    pattern: string;
    text: string;
    ours: number;
    theirs: number;
  }[];
};

function crossCheck(
  label: string,
  patterns: { pattern: string; distance: number }[],
  text: string,
  opts: {
    metric?: "levenshtein" | "damerau-levenshtein";
    normalizeDiacritics?: boolean;
    caseInsensitive?: boolean;
    wholeWords?: boolean;
  } = {},
): Result {
  const metric = opts.metric ?? "levenshtein";
  const fs = new FuzzySearch(
    patterns.map((p) => ({
      pattern: p.pattern,
      distance: p.distance,
    })),
    {
      wholeWords: opts.wholeWords ?? true,
      normalizeDiacritics:
        opts.normalizeDiacritics ?? false,
      caseInsensitive:
        opts.caseInsensitive ?? false,
      metric,
    },
  );

  const matches = fs.findIter(text);
  const result: Result = {
    label,
    total: matches.length,
    levOk: 0,
    damOk: 0,
    levFail: [],
    damFail: [],
  };

  for (const m of matches) {
    const pat = patterns[m.pattern]!.pattern;
    const matched = m.text;

    // Cross-check against js-levenshtein
    const refLev = refLevenshtein(pat, matched);
    if (
      metric === "levenshtein" &&
      m.distance === refLev
    ) {
      result.levOk++;
    } else if (metric === "levenshtein") {
      result.levFail.push({
        pattern: pat,
        text: matched,
        ours: m.distance,
        theirs: refLev,
      });
    }

    // Cross-check against damerau-levenshtein
    const refDam = refDamerau(pat, matched);
    if (
      metric === "damerau-levenshtein" &&
      m.distance === refDam
    ) {
      result.damOk++;
    } else if (metric === "damerau-levenshtein") {
      result.damFail.push({
        pattern: pat,
        text: matched,
        ours: m.distance,
        theirs: refDam,
      });
    }

    // For Levenshtein mode, also verify that
    // js-levenshtein agrees
    if (metric === "levenshtein") {
      // Our Levenshtein should match theirs
    }

    // For Damerau mode, also verify standard
    // Levenshtein is >= Damerau (invariant)
    if (metric === "damerau-levenshtein") {
      if (refLev < refDam) {
        console.log(
          `  WARNING: Levenshtein (${refLev})` +
            ` < Damerau (${refDam}) for` +
            ` "${pat}" vs "${matched}"`,
        );
      }
    }
  }

  return result;
}

function printResult(r: Result) {
  const metric =
    r.damFail.length + r.damOk > 0
      ? "damerau"
      : "levenshtein";
  const ok =
    metric === "damerau" ? r.damOk : r.levOk;
  const fails =
    metric === "damerau"
      ? r.damFail
      : r.levFail;

  const status =
    fails.length === 0
      ? "\x1b[32m✓ PASS\x1b[0m"
      : "\x1b[31m✗ FAIL\x1b[0m";

  const ref =
    metric === "damerau"
      ? "damerau-levenshtein (npm)"
      : "js-levenshtein (npm)";

  console.log(
    `  ${status} ${r.label}: ` +
      `${ok}/${r.total} match ${ref}`,
  );

  for (const f of fails.slice(0, 5)) {
    console.log(
      `    MISMATCH: "${f.pattern}" vs ` +
        `"${f.text}": ours=${f.ours}, ` +
        `${ref}=${f.theirs}`,
    );
  }
  if (fails.length > 5) {
    console.log(
      `    ... and ${fails.length - 5} more`,
    );
  }
}

// ─── Scenarios ───────────────────────────────

console.log("=".repeat(62));
console.log(" CROSS-CHECK: our distances vs npm");
console.log(
  " js-levenshtein + damerau-levenshtein",
);
console.log("=".repeat(62));

// ── 1. Synthetic text ────────────────────────

console.log("\n### Synthetic Czech (64KB)\n");

function genText(): string {
  const words = [
    "smlouva", "podepsal", "nájemní",
    "město", "okres", "pan", "paní",
    "rok", "příloha", "dodatek", "částka",
  ];
  const parts: string[] = [];
  let size = 0;
  let i = 0;
  while (size < 64 * 1024) {
    if (i % 200 === 50) parts.push("Gais1erová");
    else if (i % 200 === 100) parts.push("Nowák");
    else if (i % 200 === 150) parts.push("Pribram");
    else if (i % 300 === 200) parts.push("Nvoák");
    else if (i % 300 === 250) parts.push("Dvorak");
    else parts.push(words[i % words.length]!);
    size += parts.at(-1)!.length + 1;
    i++;
  }
  return parts.join(" ");
}

const czText = genText();
const CZ = [
  { pattern: "Gaislerová", distance: 1 },
  { pattern: "Novák", distance: 1 },
  { pattern: "Šnytrová", distance: 1 },
  { pattern: "Příbram", distance: 2 },
  { pattern: "Dvořák", distance: 1 },
];

printResult(
  crossCheck(
    "Levenshtein, wholeWords",
    CZ,
    czText,
  ),
);

printResult(
  crossCheck(
    "Damerau-Levenshtein, wholeWords",
    CZ,
    czText,
    { metric: "damerau-levenshtein" },
  ),
);

printResult(
  crossCheck(
    "Levenshtein, no wholeWords",
    CZ,
    czText,
    { wholeWords: false },
  ),
);

printResult(
  crossCheck(
    "Damerau, no wholeWords",
    CZ,
    czText,
    {
      metric: "damerau-levenshtein",
      wholeWords: false,
    },
  ),
);

// ── 2. Canterbury bible.txt ──────────────────

const CORPUS = join(__dirname, "corpus");
const load = (name: string): string => {
  try {
    return readFileSync(
      join(CORPUS, name),
      "utf-8",
    );
  } catch {
    return "";
  }
};

const bible = load("bible.txt");
if (bible) {
  console.log(
    `\n### Canterbury bible.txt (${(bible.length / 1e6).toFixed(1)} MB)\n`,
  );

  const BIBLE = [
    { pattern: "covenant", distance: 1 },
    { pattern: "Jerusalem", distance: 1 },
    { pattern: "Abraham", distance: 1 },
    { pattern: "Pharaoh", distance: 1 },
    { pattern: "offering", distance: 1 },
  ];

  printResult(
    crossCheck(
      "Levenshtein dist 1",
      BIBLE,
      bible,
    ),
  );
  printResult(
    crossCheck(
      "Levenshtein dist 2",
      BIBLE.map((p) => ({ ...p, distance: 2 })),
      bible,
    ),
  );
  printResult(
    crossCheck(
      "Damerau dist 1",
      BIBLE,
      bible,
      { metric: "damerau-levenshtein" },
    ),
  );
  printResult(
    crossCheck(
      "Damerau dist 2",
      BIBLE.map((p) => ({ ...p, distance: 2 })),
      bible,
      { metric: "damerau-levenshtein" },
    ),
  );
}

// ── 3. Leipzig Czech news ────────────────────

const cesNews = load("ces_news_2024_300K.txt");
if (cesNews) {
  console.log(
    `\n### Leipzig Czech news (${(cesNews.length / 1e6).toFixed(1)} MB)\n`,
  );

  const CZ_NEWS = [
    { pattern: "Babiš", distance: 1 },
    { pattern: "Praha", distance: 1 },
    { pattern: "vláda", distance: 1 },
    { pattern: "soudní", distance: 1 },
    { pattern: "koalice", distance: 1 },
  ];

  printResult(
    crossCheck(
      "Levenshtein dist 1",
      CZ_NEWS,
      cesNews,
    ),
  );
  printResult(
    crossCheck(
      "Damerau dist 1",
      CZ_NEWS,
      cesNews,
      { metric: "damerau-levenshtein" },
    ),
  );
}

// ── 4. Random pairs (stress) ─────────────────

console.log("\n### Random pairs (10,000 × 2)\n");

function randomStr(len: number): string {
  const chars =
    "abcdefghijklmnopqrstuvwxyz" +
    "áčďéěíňóřšťúůýž";
  let s = "";
  for (let i = 0; i < len; i++) {
    s += chars[Math.floor(
      Math.random() * chars.length,
    )];
  }
  return s;
}

let levOk = 0;
let levTotal = 0;
let damOk = 0;
let damTotal = 0;
const levMismatches: string[] = [];
const damMismatches: string[] = [];

for (let i = 0; i < 10000; i++) {
  const pat = randomStr(4 + (i % 8));
  const hay = randomStr(6 + (i % 20));

  // Levenshtein check
  const fsLev = new FuzzySearch(
    [{ pattern: pat, distance: 2 }],
    { wholeWords: false, metric: "levenshtein" },
  );
  for (const m of fsLev.findIter(hay)) {
    levTotal++;
    const ref = refLevenshtein(pat, m.text);
    if (m.distance === ref) {
      levOk++;
    } else {
      levMismatches.push(
        `"${pat}" vs "${m.text}": ` +
          `ours=${m.distance}, npm=${ref}`,
      );
    }
  }

  // Damerau check
  const fsDam = new FuzzySearch(
    [{ pattern: pat, distance: 2 }],
    {
      wholeWords: false,
      metric: "damerau-levenshtein",
    },
  );
  for (const m of fsDam.findIter(hay)) {
    damTotal++;
    const ref = refDamerau(pat, m.text);
    if (m.distance === ref) {
      damOk++;
    } else {
      damMismatches.push(
        `"${pat}" vs "${m.text}": ` +
          `ours=${m.distance}, npm=${ref}`,
      );
    }
  }
}

const levStatus =
  levMismatches.length === 0
    ? "\x1b[32m✓ PASS\x1b[0m"
    : "\x1b[31m✗ FAIL\x1b[0m";
console.log(
  `  ${levStatus} Levenshtein: ` +
    `${levOk}/${levTotal} match js-levenshtein`,
);
for (const m of levMismatches.slice(0, 5)) {
  console.log(`    MISMATCH: ${m}`);
}

const damStatus =
  damMismatches.length === 0
    ? "\x1b[32m✓ PASS\x1b[0m"
    : "\x1b[31m✗ FAIL\x1b[0m";
console.log(
  `  ${damStatus} Damerau: ` +
    `${damOk}/${damTotal} match ` +
    `damerau-levenshtein (npm)`,
);
for (const m of damMismatches.slice(0, 5)) {
  console.log(`    MISMATCH: ${m}`);
}

console.log("\n" + "=".repeat(62));
console.log(" Done.");
console.log("=".repeat(62));
