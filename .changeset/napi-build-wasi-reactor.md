---
"@stll/fuzzy-search": patch
---

Build the WebAssembly binding with napi-build 2.6.0 and the emnapi 2.0.0-beta.2 runtime. napi-build now finds the WASI reactor startup object through `rustc --print sysroot`, so the build links it wherever Cargo runs `rustc` from `PATH`. The WebAssembly packages depend on `@emnapi/core` and `@emnapi/runtime` 2.0.0-beta.2, which the binding now links against.
