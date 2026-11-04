---
"@stll/fuzzy-search": patch
---

Move the WebAssembly binding to the emnapi 2 runtime (2.0.0-beta.2, a prerelease) with napi-build 2.6.0, which finds the WASI reactor startup object through `rustc --print sysroot`. The WebAssembly packages now depend on `@emnapi/core` and `@emnapi/runtime` 2.0.0-beta.2 and pin `@napi-rs/wasm-runtime` to 1.2.6, whose async-work and thread-safe-function plugins read the emnapi 2 module context in worker threads. The generated Node and browser loaders are regenerated for emnapi 2: the bundled async-work and thread-safe-function plugins receive the emnapi 2 module context, cleanup polling rejects instead of hanging when scheduling throws, and browser workers keep static `{ type: "module" }` options so bundlers such as Vite can resolve them.
