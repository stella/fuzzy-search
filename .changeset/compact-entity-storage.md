---
"@stll/fuzzy-search": minor
---

Reduce EntityIndex retained native memory with bit-packed posting ids, shared posting payloads, and flat forward arrays that share position weights and join layouts. Each form preserves its complete position weight and minimum-weight sequence; identical sequences share storage. Serialization version 2 rejects older versions with an EntityIndexUnsupportedVersion error.
