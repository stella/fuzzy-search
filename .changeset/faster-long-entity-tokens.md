---
"@stll/fuzzy-search": patch
---

Speed up entity searches for long tokens by keeping the common narrow edit-distance rows inline and storing their bounded values compactly during vocabulary traversal. Search results and deterministic work accounting are unchanged.
