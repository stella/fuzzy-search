---
"@stll/fuzzy-search": patch
---

Speed up entity searches for long tokens by keeping the common narrow edit-distance rows inline instead of allocating them during vocabulary traversal. Search results and deterministic work accounting are unchanged.
