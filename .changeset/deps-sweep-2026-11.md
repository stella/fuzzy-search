---
"@stll/fuzzy-search": patch
---

Build the native binding with napi 3.16 and move the WebAssembly packages to `@emnapi/core` and `@emnapi/runtime` 2.0.0-beta.3 with `@napi-rs/wasm-runtime` 1.2.7. `EntityIndexBuilder` calls made while `finishAsync` is pending keep reporting that the builder was consumed.
