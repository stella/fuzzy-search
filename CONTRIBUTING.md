# Contributing

Thank you for your interest in contributing to
`@stll/fuzzy-search`.

## CLA

All contributors must sign the
[Contributor License Agreement](https://github.com/stella/cla/blob/main/CLA.md).
You will be prompted automatically when you open
a pull request.

## Development setup

```bash
# Prerequisites: Rust toolchain, Bun
bun install
bun run build       # native module
bun test            # run tests
bun run lint        # oxlint
bun run format      # oxfmt + rustfmt
```

## Architecture

The package ships native binaries for each platform as
optional npm sub-packages (`npm/<target>/`). On install,
npm/bun only downloads the binary matching the host platform.

For browser/WASM support, the `npm/wasm32-wasi/` sub-package
contains the WASM binary and WASI runtime glue. The umbrella
package's `browser` export condition points to `dist/wasm.js`,
which imports from `@stll/fuzzy-search-wasm32-wasi`.

### Building WASM locally

```bash
# Requires: wasm32-wasip1-threads Rust target
rustup target add wasm32-wasip1-threads
bun run build:wasm
# Copy artifacts into the sub-package
cp fuzzy-search.wasm \
  npm/wasm32-wasi/fuzzy-search.wasm32-wasi.wasm
cp fuzzy-search.wasi.cjs npm/wasm32-wasi/
cp fuzzy-search.wasi-browser.js npm/wasm32-wasi/
cp wasi-worker.mjs npm/wasm32-wasi/
cp wasi-worker-browser.mjs npm/wasm32-wasi/
```

## Pull requests

- One logical change per PR.
- Include tests for bug fixes and new features.
- Run `bun test && bun run lint && bun run format`
  before submitting.
- Use [Conventional Commits](https://www.conventionalcommits.org/):
  `feat:`, `fix:`, `chore:`, `docs:`.
- Squash merge is enforced; keep the PR title clean.

## Benchmarks

If your change affects performance, include
benchmark results:

```bash
bun run bench:install   # one-time
bun run bench:download  # one-time
bun run bench:correctness
bun run bench:speed
```

## Reporting issues

Open a [GitHub issue](https://github.com/stella/fuzzy-search/issues).
For security vulnerabilities, see
[SECURITY.md](./SECURITY.md).
