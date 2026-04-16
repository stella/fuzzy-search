<p align="center">
  <img src=".github/assets/banner.png" alt="Stella" width="100%" />
</p>

# @stll/fuzzy-search

[NAPI-RS](https://napi.rs/) approximate substring
matching for Node.js and Bun. Finds near-matches
within edit distance k with stable UTF-16 offsets,
replace-safe match ranges, and optional diacritics
normalization.

Built on [Myers' bit-parallel algorithm](https://doi.org/10.1145/316542.316550)
(1999), implemented in Rust and exposed to
JavaScript via [NAPI-RS](https://github.com/napi-rs/napi-rs).

## Install

```bash
npm install @stll/fuzzy-search
# or
bun add @stll/fuzzy-search
```

The companion `@stll/fuzzy-search-wasm` package is
available for browser builds.

If you use the browser package with Vite, import the
bundled plugin so the generated WASM loader is not
pre-bundled into broken asset URLs:

```typescript
import { defineConfig } from "vite";
import stllFuzzySearchWasm from "@stll/fuzzy-search-wasm/vite";

export default defineConfig({
  plugins: [stllFuzzySearchWasm()],
});
```

GitHub releases include npm tarballs, an SBOM, and
third-party notices.

Prebuilts are available for:

| Platform      | Architecture |
| ------------- | ------------ |
| macOS         | x64, arm64   |
| Linux (glibc) | x64, arm64   |
| WASM          | browser      |

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

### Distance helper

```typescript
import { distance } from "@stll/fuzzy-search";

distance("kitten", "sitting"); // 3
distance("abcd", "abdc", "damerau-levenshtein"); // 1
```

## Benchmarks

The repository includes a checked-in benchmark harness
for synthetic and corpus-based searches. The inputs
are public and the scripts are reproducible from the
repo. Run them locally:

```bash
bun run bench:install
bun run bench:download
bun run bench:speed
bun run bench:correctness
```

The speed harness compares practical JS ecosystem
alternatives, but not every comparator implements the
same exact semantics. `@stll/fuzzy-search` is solving
approximate substring search with offsets and
replacement-friendly match ranges; tools like
`fuse.js` and `fuzzball` are included as reference
points, not as exact drop-in equivalents. The
headline comparisons in this repo are the
substring-mode rows against sliding-window
Levenshtein baselines.

Representative baseline from the checked-in public
harness on this machine:

- runtime: Bun `1.3.12`
- platform: macOS `26.4.1` (`Darwin arm64`)

| Scenario                         | `@stll/fuzzy-search` | Sliding-window JS baseline | Relative |
| -------------------------------- | -------------------- | -------------------------- | -------- |
| Czech legal, `64 KB`, `5` names  | `2.41 ms`            | `80.78 ms`                 | `33.5x`  |
| Bible, `4.0 MB`, `5` names       | `239.91 ms`          | `3903.26 ms`               | `16.3x`  |
| Czech news, `4.8 MB`, `5` names  | `262.39 ms`          | `4350.52 ms`               | `16.6x`  |
| German news, `5.5 MB`, `5` names | `405.72 ms`          | `6816.03 ms`               | `16.8x`  |

These rows are substring mode (`wholeWords: false`)
with edit distance `1-2`, which is the core workload
this package is designed for.

<details>
<summary>Alternatives tested</summary>

- [fastest-levenshtein](https://www.npmjs.com/package/fastest-levenshtein) + sliding window — fastest JS Levenshtein distance
- [fuse.js](https://www.npmjs.com/package/fuse.js) — fuzzy search (scoring, not substring matching)
- [fuzzball](https://www.npmjs.com/package/fuzzball) — Python rapidfuzz port
- naive JS — O(nm) Levenshtein per window position

</details>

## Correctness

Correctness is covered by example-based tests and
property tests. The property suite verifies distance
bounds, oracle agreement, whole-word boundaries,
UTF-16 offset stability, normalization behavior, and
mixed option combinations over randomized inputs.

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
  prefiltering and fuzzy-search on flagged regions.
- **WASM requires `SharedArrayBuffer`.** Browser
  builds need `Cross-Origin-Opener-Policy: same-origin`
  and `Cross-Origin-Embedder-Policy: require-corp`
  headers.

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
