# Changelog

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
