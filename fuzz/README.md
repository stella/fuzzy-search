# Entity lexicon fuzzing

Run from the repository root with nightly Rust and cargo-fuzz installed:

```sh
cargo +nightly fuzz run entity_lexicon -- -max_total_time=60
```

The target compares complete lookup results against the public OSA distance
function over bounded Unicode strings, then checks exact-first and bigram/id
ordering. Input is newline-separated: query first, followed by distinct tokens.

Run the shared-prefix lookup benchmark with:

```sh
cargo bench --package stella-fuzzy-search-core --bench entity_lexicon
```
