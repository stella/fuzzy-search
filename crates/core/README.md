# stella-fuzzy-search-core

Rust core for [`@stll/fuzzy-search`](https://www.npmjs.com/package/@stll/fuzzy-search).
It provides Unicode-aware approximate substring matching with stable byte and UTF-16
offsets, replace-safe spans, and optional diacritic normalization.

```rust
use stella_fuzzy_search_core::{FuzzySearch, Options, PatternEntry};

let matcher = FuzzySearch::new(
  vec![PatternEntry {
    pattern: "Gaislerová".into(),
    distance: Some(1),
  }],
  Options::builder()
    .normalize_diacritics(true)
    .build(),
)?;

assert!(matcher.is_match("Smlouva s Gais1erová")?);
# Ok::<(), stella_fuzzy_search_core::Error>(())
```

The JavaScript and browser packages, usage guide, and development documentation
live in the [repository](https://github.com/stella/fuzzy-search).

## License

MIT
