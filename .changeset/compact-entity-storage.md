---
"@stll/fuzzy-search": minor
---

Reduce EntityIndex retained native memory with bit-packed posting ids, shared posting payloads, and flat forward arrays that share position weights and join layouts. Each form preserves its complete position weight and minimum-weight sequence; identical sequences share storage. Version 1 payloads remain readable; new payloads use version 2.
