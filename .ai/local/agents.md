## Repository Specifics

`@stll/fuzzy-search` is a Node/Bun package backed by a Rust Myers approximate substring engine, with native and WASM package outputs.

### Commands

- `bun install`
- `bun run lint`
- `bun run typecheck`
- `bun test`
- `bun run test:props`
- `bun run test:runtime:bun`
- `bun run test:runtime:node`
- `bun run build:js`
- `bun run version:check`

### Native Package Rules

- Keep approximate-match semantics, offsets, returned match text, and replace-safe spans consistent across native, WASM, Bun, and Node runtimes.
- Use property tests for edit-distance boundaries and regression tests for Unicode and overlapping-match behavior.
- Avoid changing package artifact layout unless release packaging is the target.
