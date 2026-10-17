---
"@stll/fuzzy-search": minor
---

Tighten EntityIndex threshold retrieval for dense common-token indexes by deduplicating remaining query coverage across spelling forms and joined units, retiring exhausted posting cursors, and bounding candidate lookup cost independently of fuzzy vocabulary match counts.
