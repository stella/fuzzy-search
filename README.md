<p align="center">
  <img src=".github/assets/banner.png" alt="Stella" width="100%" />
</p>

# @stll/fuzzy-search

[NAPI-RS](https://napi.rs/) fuzzy string matching
for Node.js and Bun. Finds approximate occurrences
of patterns within edit distance k, immune to typos,
OCR errors, and diacritics variants.

Built on [Myers' bit-parallel algorithm](https://doi.org/10.1145/316542.316550)
(1999), implemented in Rust and exposed to
JavaScript via [NAPI-RS](https://github.com/napi-rs/napi-rs).

## Install

```bash
npm install @stll/fuzzy-search
# or
bun add @stll/fuzzy-search
```

Prebuilt binaries are available for:

| Platform      | Architecture |
| ------------- | ------------ |
| macOS         | x64, arm64   |
| Linux (glibc) | x64, arm64   |
| Linux (musl)  | x64          |
| Windows       | x64          |

## Usage

```typescript
import { FuzzySearch } from "@stll/fuzzy-search";

const fs = new FuzzySearch(
  [
    { pattern: "Gaislerová", distance: 1 },
    { pattern: "Novák", distance: 1 },
    { pattern: "Příbram", distance: 2 },
  ],
  {
    normalizeDiacritics: true,
    wholeWords: true,
  },
);

fs.findIter("Smlouva s Gais1erová v Pribram");
// [
//   { pattern: 0, start: 10, end: 20,
//     text: "Gais1erová", distance: 1 },
//   { pattern: 2, start: 23, end: 30,
//     text: "Pribram", distance: 0 },
// ]
```

### Patterns

Patterns can be strings (default distance 1) or
objects with explicit distance and optional name:

```typescript
const fs = new FuzzySearch([
  "simple", // distance 1
  { pattern: "named", name: "entity" }, // distance 1
  { pattern: "precise", distance: 2 }, // distance 2
]);
```

Distance must be less than pattern length.

### Options

```typescript
const fs = new FuzzySearch(patterns, {
  // Strip diacritics before matching (NFD + remove
  // combining marks). "Příbram" matches "Pribram"
  // at distance 0.
  normalizeDiacritics: true, // default: false

  // Only match whole words. Uses Unicode
  // is_alphanumeric() for boundary detection.
  // CJK characters always pass (no inter-word
  // spaces in CJK).
  wholeWords: true, // default: true

  // Case-insensitive matching (Unicode-aware).
  caseInsensitive: true, // default: false

  // Unicode word boundaries (reserved for future
  // UAX#29 segmentation support).
  unicodeBoundaries: true, // default: true
});
```

### Replace

```typescript
fs.replaceAll("Smlouva s Gais1erová", [
  "[REDACTED]",
  "[REDACTED]",
  "[REDACTED]",
]);
// "Smlouva s [REDACTED]"
```

`replacements[i]` replaces pattern `i`.

## Benchmarks

Measured on Apple M3, 24 GB RAM, macOS 25.3.0,
Bun 1.3.10. Search-only times, automaton pre-built.

### Synthetic legal text (64KB, 5 patterns, dist 1-2)

| Library                      | Time       | Speedup |
| ---------------------------- | ---------- | ------- |
| **@stll/fuzzy-search**       | **2.3 ms** | —       |
| fuzzball.extract             | 9.2 ms     | 3.9x    |
| fuse.js (word-split)         | 57 ms      | 25x     |
| fastest-levenshtein + window | 82 ms      | 35x     |
| naive JS (sliding window)    | 511 ms     | 219x    |

### Real corpus (Canterbury bible.txt, 4.0 MB)

| Library                      | Time       | Speedup |
| ---------------------------- | ---------- | ------- |
| **@stll/fuzzy-search**       | **258 ms** | —       |
| fastest-levenshtein + window | 4,249 ms   | 16.5x   |

### Real corpus (Leipzig Czech news, 4.8 MB)

| Library                      | Time       | Speedup |
| ---------------------------- | ---------- | ------- |
| **@stll/fuzzy-search**       | **249 ms** | —       |
| fastest-levenshtein + window | 4,147 ms   | 16.6x   |

Run locally:
`bun run bench:install && bun run bench:download && bun run bench:speed`

<details>
<summary>Alternatives tested</summary>

- [fastest-levenshtein](https://www.npmjs.com/package/fastest-levenshtein) + sliding window — fastest JS Levenshtein distance
- [fuse.js](https://www.npmjs.com/package/fuse.js) — fuzzy search (scoring, not substring matching)
- [fuzzball](https://www.npmjs.com/package/fuzzball) — Python rapidfuzz port
- naive JS — O(nm) Levenshtein per window position

</details>

## Correctness

Every match is verified against a naive Levenshtein
oracle:

- **36 property tests** × 1,000 random inputs =
  36,000 test cases (~9,000 assertions).
- **25,528 matches** oracle-verified on real corpora
  (Canterbury bible.txt, Leipzig Czech news) across
  all option combinations.
- **9 bugs found and fixed** by property tests.

Properties include: distance correctness (oracle),
non-overlapping, monotonic offsets, wholeWords
boundaries, normalization idempotence, full cartesian
product of all option combinations × distances,
UTF-16 supplementary plane, CJK text, long patterns
(50-63 chars), duplicate/substring patterns.

## API

| Method                                | Returns        | Description              |
| ------------------------------------- | -------------- | ------------------------ |
| `new FuzzySearch(patterns, options?)` | instance       | Build matcher            |
| `.findIter(haystack)`                 | `FuzzyMatch[]` | Non-overlapping matches  |
| `.isMatch(haystack)`                  | `boolean`      | Any pattern matches?     |
| `.replaceAll(haystack, replacements)` | `string`       | Replace matched patterns |
| `.patternCount`                       | `number`       | Number of patterns       |

### Types

```typescript
type PatternEntry =
  | string
  | { pattern: string; distance?: number; name?: string };

type Options = {
  normalizeDiacritics?: boolean; // default: false
  wholeWords?: boolean; // default: true
  caseInsensitive?: boolean; // default: false
  unicodeBoundaries?: boolean; // default: true
};

type FuzzyMatch = {
  pattern: number; // index into patterns array
  start: number; // UTF-16 code unit offset
  end: number; // exclusive
  text: string; // matched substring
  distance: number; // actual Levenshtein distance
  name?: string; // pattern name (if provided)
};
```

Match offsets are UTF-16 code unit indices,
compatible with `String.prototype.slice()`.

### Error handling

- Constructor throws if a pattern is empty, longer
  than 64 characters, or has distance >= pattern
  length.
- `replaceAll` throws if `replacements.length`
  does not equal `patternCount`.

## How it works

1. **Myers' bit-parallel algorithm** scans the text
   in O(n) per pattern for patterns up to 64
   characters. No DFA construction, no state
   explosion at higher distances.

2. **Start position recovery** via small-window
   Levenshtein: for each match end position from
   Myers, a window of [m-k, m+k] characters is
   evaluated to find the exact start and distance.

3. **Diacritics normalization**: NFD decomposition +
   combining mark stripping (Unicode General
   Category M via `unicode-normalization` crate).
   Covers all scripts.

4. **UTF-16 offset translation**: character-level
   matching with incremental char→UTF-16 mapping
   for JS string compatibility.

## Limitations

- **Pattern length capped at 64 characters.** Myers
  uses a single u64 bit-vector per pattern. Longer
  patterns would need multi-word vectors (not yet
  implemented).
- **No streaming API.** The full haystack must be in
  memory. For chunked processing, use
  `@stll/aho-corasick`'s `StreamMatcher` for exact
  matches and fuzzy-search on flagged regions.
- **WASM requires `SharedArrayBuffer`.** Browser
  builds need `Cross-Origin-Opener-Policy: same-origin`
  and `Cross-Origin-Embedder-Policy: require-corp`
  headers.

### Using with Vite

Vite's dependency pre-bundler rewrites
`import.meta.url`, which breaks the relative
`.wasm` path emitted by the napi-rs loader. Import
the bundled plugin so the package is excluded from
pre-bundling:

```ts
// vite.config.ts
import stllWasm from "@stll/fuzzy-search-wasm/vite";

export default {
  plugins: [stllWasm()],
};
```

## Development

```bash
bun install
bun run build           # native module (requires Rust)
bun test                # 36 unit tests
bun run test:props      # 36 property tests × 1000 runs

bun run bench:install   # benchmark dependencies
bun run bench:download  # download corpora
bun run bench:speed     # speed comparison
bun run bench:correctness  # oracle verification

bun run lint            # oxlint
bun run format          # oxfmt + rustfmt
```

## License

[MIT](./LICENSE)
