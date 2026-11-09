---
"@stll/fuzzy-search": patch
---

Build the native binding with napi 3.16 and napi-derive 3.7. `EntityIndexBuilder` calls made while `finishAsync` is pending keep reporting that the builder was consumed.
