# Changelog

## 1.1.4

### Patch Changes

- [#106](https://github.com/stella/fuzzy-search/pull/106) [`e5230d9`](https://github.com/stella/fuzzy-search/commit/e5230d99861ad3334c4cb2aa7f3201fe47253ed7) Thanks [@jan-kubica](https://github.com/jan-kubica)! - Refresh dependencies and expose a compile-time checked Rust options builder.

## 1.1.2

### Features

- Windows support: publish the
  `@stll/fuzzy-search-win32-x64-msvc` native binding
  (x86_64). The loader already resolved it; the
  package now exists on the registry.

## 1.1.0

### Features

- `FuzzyMatch.score`: normalized similarity in
  `[0, 1]`, computed as `1 - distance / pattern.length`
  and clamped at 0. Always populated.
- `Options.minScore`: drop matches whose score is
  below the threshold (inclusive comparison).
  Applies to `findIter` only.
- `Options.kBest`: return only the top `k` matches
  across the entire haystack, ranked by score
  descending (ties broken by `start`, then pattern
  index). Applies to `findIter` only.

`replaceAll` is unchanged: every distance-qualified
match is still replaced. The new options apply on
the JS side only; the native Rust binding is
unaffected.

## 0.1.0 (2026-03-22)

### Features

- NAPI-RS bindings for Myers' bit-parallel fuzzy matching
- Levenshtein distance computation
- Fuzzy search with configurable max distance
- Batch matching across multiple patterns
- Case-insensitive matching support
- Unicode-aware string comparison
