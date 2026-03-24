/**
 * Correctness verification on real corpora.
 *
 * Every match from @stll/fuzzy-search is verified
 * against a naive Levenshtein oracle. Any
 * disagreement is a bug.
 *
 * Run: bun run bench:correctness
 * Download corpora first: bun run bench:download
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { FuzzySearch } from "../src/lib";

// ─── Levenshtein oracle ──────────────────────

function levenshtein(
  a: string,
  b: string,
): number {
  const m = a.length;
  const n = b.length;
  if (m === 0) return n;
  if (n === 0) return m;
  let prev = Array.from(
    { length: n + 1 },
    (_, i) => i,
  );
  for (let i = 1; i <= m; i++) {
    const curr = new Array<number>(n + 1);
    curr[0] = i;
    for (let j = 1; j <= n; j++) {
      const cost =
        a[i - 1] === b[j - 1] ? 0 : 1;
      curr[j] = Math.min(
        curr[j - 1]! + 1,
        prev[j]! + 1,
        prev[j - 1]! + cost,
      );
    }
    prev = curr;
  }
  return prev[n]!;
}

// ─── Corpus loader ───────────────────────────

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

// ─── Verify function ─────────────────────────

type VerifyResult = {
  totalMatches: number;
  verified: number;
  failures: {
    pattern: string;
    text: string;
    reportedDist: number;
    actualDist: number;
    start: number;
    end: number;
  }[];
  timeMs: number;
};

function verify(
  _label: string,
  patterns: { pattern: string; distance: number }[],
  text: string,
  opts: {
    wholeWords?: boolean;
    normalizeDiacritics?: boolean;
    caseInsensitive?: boolean;
  } = {},
): VerifyResult {
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
    },
  );

  const t0 = performance.now();
  const matches = fs.findIter(text);
  const searchMs = performance.now() - t0;

  const failures: VerifyResult["failures"] = [];
  let verified = 0;

  for (const m of matches) {
    // 1. text field matches slice
    const sliced = text.slice(m.start, m.end);
    if (sliced !== m.text) {
      failures.push({
        pattern: patterns[m.pattern]!.pattern,
        text: m.text,
        reportedDist: m.distance,
        actualDist: -1,
        start: m.start,
        end: m.end,
      });
      continue;
    }

    // 2. Levenshtein distance matches.
    // If normalization options are enabled, the
    // library matches on normalized text. Apply
    // the same normalization to the oracle.
    let pat = patterns[m.pattern]!.pattern;
    let matchedText = m.text;
    if (opts.normalizeDiacritics) {
      pat = pat
        .normalize("NFD")
        .replace(/[\u0300-\u036f]/g, "");
      matchedText = matchedText
        .normalize("NFD")
        .replace(/[\u0300-\u036f]/g, "");
    }
    if (opts.caseInsensitive) {
      pat = pat.toLowerCase();
      matchedText = matchedText.toLowerCase();
    }
    const maxDist = patterns[m.pattern]!.distance;
    const actualDist = levenshtein(
      pat,
      matchedText,
    );

    if (actualDist !== m.distance) {
      failures.push({
        pattern: pat,
        text: m.text,
        reportedDist: m.distance,
        actualDist,
        start: m.start,
        end: m.end,
      });
      continue;
    }

    // 3. Distance within bound
    if (actualDist > maxDist) {
      failures.push({
        pattern: pat,
        text: m.text,
        reportedDist: m.distance,
        actualDist,
        start: m.start,
        end: m.end,
      });
      continue;
    }

    verified++;
  }

  return {
    totalMatches: matches.length,
    verified,
    failures,
    timeMs: searchMs,
  };
}

function printResult(
  label: string,
  result: VerifyResult,
) {
  const status =
    result.failures.length === 0
      ? "\x1b[32m✓ PASS\x1b[0m"
      : "\x1b[31m✗ FAIL\x1b[0m";
  console.log(
    `  ${status} ${label}: ` +
      `${result.verified}/${result.totalMatches}` +
      ` verified (${result.timeMs.toFixed(1)} ms)`,
  );
  for (const f of result.failures.slice(0, 5)) {
    console.log(
      `    FAIL: "${f.pattern}" matched ` +
        `"${f.text}" at ${f.start}..${f.end}, ` +
        `reported dist=${f.reportedDist}, ` +
        `actual=${f.actualDist}`,
    );
  }
  if (result.failures.length > 5) {
    console.log(
      `    ... and ${result.failures.length - 5} more`,
    );
  }
}

// ─── Scenarios ───────────────────────────────

console.log("=".repeat(62));
console.log(" CORRECTNESS VERIFICATION");
console.log(
  " Every match checked against Levenshtein oracle",
);
console.log("=".repeat(62));

// ── 1. Synthetic Czech legal text ────────────

console.log(
  "\n### Synthetic Czech legal text (64KB)\n",
);

function generateCzechText(): string {
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
  let i = 0;
  while (size < 64 * 1024) {
    if (i % 200 === 50)
      parts.push("Gais1erová");
    else if (i % 200 === 100)
      parts.push("Nowák");
    else if (i % 200 === 150)
      parts.push("Pribram");
    else if (i % 500 === 250)
      parts.push("Dvorak");
    else if (i % 500 === 350)
      parts.push("Snytrova");
    else parts.push(words[i % words.length]!);
    size += parts.at(-1)!.length + 1;
    i++;
  }
  return parts.join(" ");
}

const czText = generateCzechText();

const CZECH_PATTERNS = [
  { pattern: "Gaislerová", distance: 1 },
  { pattern: "Novák", distance: 1 },
  { pattern: "Šnytrová", distance: 1 },
  { pattern: "Příbram", distance: 2 },
  { pattern: "Dvořák", distance: 1 },
];

printResult(
  "dist 1-2, wholeWords",
  verify("czech-ww", CZECH_PATTERNS, czText),
);

printResult(
  "dist 1-2, no wholeWords",
  verify("czech-no-ww", CZECH_PATTERNS, czText, {
    wholeWords: false,
  }),
);

printResult(
  "dist 1-2, normalizeDiacritics",
  verify("czech-norm", CZECH_PATTERNS, czText, {
    normalizeDiacritics: true,
  }),
);

printResult(
  "dist 1-2, caseInsensitive",
  verify(
    "czech-ci",
    CZECH_PATTERNS,
    czText.toUpperCase(),
    { caseInsensitive: true },
  ),
);

// ── 2. Canterbury bible.txt ──────────────────

const bible = load("bible.txt");
if (bible) {
  console.log(
    `\n### Canterbury bible.txt (${(bible.length / 1e6).toFixed(1)} MB)\n`,
  );

  const BIBLE_PATTERNS = [
    { pattern: "covenant", distance: 1 },
    { pattern: "Jerusalem", distance: 1 },
    { pattern: "Abraham", distance: 1 },
    { pattern: "Pharaoh", distance: 1 },
    { pattern: "offering", distance: 1 },
  ];

  printResult(
    "5 names dist 1, wholeWords",
    verify("bible-ww", BIBLE_PATTERNS, bible),
  );

  printResult(
    "5 names dist 2, wholeWords",
    verify(
      "bible-d2",
      BIBLE_PATTERNS.map((p) => ({
        ...p,
        distance: 2,
      })),
      bible,
    ),
  );

  printResult(
    "5 names dist 1, caseInsensitive",
    verify("bible-ci", BIBLE_PATTERNS, bible, {
      caseInsensitive: true,
    }),
  );

  printResult(
    "5 names dist 1, norm + CI + WW",
    verify(
      "bible-all",
      BIBLE_PATTERNS,
      bible,
      {
        normalizeDiacritics: true,
        caseInsensitive: true,
        wholeWords: true,
      },
    ),
  );

  printResult(
    "5 names dist 1, no wholeWords",
    verify(
      "bible-no-ww",
      BIBLE_PATTERNS,
      bible,
      { wholeWords: false },
    ),
  );

  printResult(
    "5 names dist 4, wholeWords",
    verify(
      "bible-d4",
      BIBLE_PATTERNS.map((p) => ({
        ...p,
        distance: 4,
      })),
      bible,
    ),
  );
} else {
  console.log(
    "\n### Canterbury bible.txt (not downloaded)\n",
  );
  console.log(
    "  Run: bun run bench:download",
  );
}

// ── 3. Leipzig Czech news ────────────────────

const cesNews = load("ces_news_2024_300K.txt");
if (cesNews) {
  console.log(
    `\n### Leipzig Czech news (${(cesNews.length / 1e6).toFixed(1)} MB)\n`,
  );

  const CZ_NEWS_PATTERNS = [
    { pattern: "Babiš", distance: 1 },
    { pattern: "Praha", distance: 1 },
    { pattern: "vláda", distance: 1 },
    { pattern: "soudní", distance: 1 },
    { pattern: "koalice", distance: 1 },
  ];

  printResult(
    "5 CZ news names dist 1",
    verify(
      "ces-news",
      CZ_NEWS_PATTERNS,
      cesNews,
    ),
  );

  printResult(
    "5 CZ names, normalizeDiacritics",
    verify(
      "ces-norm",
      CZ_NEWS_PATTERNS,
      cesNews,
      { normalizeDiacritics: true },
    ),
  );

  printResult(
    "5 CZ names, caseInsensitive",
    verify(
      "ces-ci",
      CZ_NEWS_PATTERNS,
      cesNews,
      { caseInsensitive: true },
    ),
  );
} else {
  console.log(
    "\n### Leipzig Czech news (not downloaded)\n",
  );
  console.log(
    "  Run: bun run bench:download",
  );
}

// ── 4. Leipzig German news ───────────────────

const deuNews = load("deu_news_2024_300K.txt");
if (deuNews) {
  console.log(
    `\n### Leipzig German news (${(deuNews.length / 1e6).toFixed(1)} MB)\n`,
  );

  const DE_PATTERNS = [
    { pattern: "München", distance: 1 },
    { pattern: "Straße", distance: 1 },
    { pattern: "Gesellschaft", distance: 1 },
    { pattern: "Regierung", distance: 1 },
    { pattern: "Unternehmen", distance: 1 },
  ];

  printResult(
    "5 DE news names dist 1",
    verify("deu-news", DE_PATTERNS, deuNews),
  );

  printResult(
    "5 DE names, normalizeDiacritics",
    verify("deu-norm", DE_PATTERNS, deuNews, {
      normalizeDiacritics: true,
    }),
  );
} else {
  console.log(
    "\n### Leipzig German news (not downloaded)\n",
  );
  console.log(
    "  Run: bun run bench:download",
  );
}

// ── Summary ──────────────────────────────────

console.log("\n" + "=".repeat(62));
console.log(" Done.");
console.log("=".repeat(62));
