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
bun run build:js    # package ESM bundles
bun test            # run tests
bun run test:node   # Node ESM smoke test
bun run lint        # oxlint
bun run format      # oxfmt + rustfmt
```

## Architecture

The package ships native binaries for each platform as
optional npm sub-packages (`npm/<target>/`). On install,
npm/bun only downloads the binary matching the host platform.

Browser/WASM support is published as a separate package,
`@stll/fuzzy-search-wasm` (source in `wasm/package.json`).
Its entry point is `wasm/dist/wasm.mjs`, which imports from
`@stll/fuzzy-search-wasm32-wasi`. The root `tsdown` config
builds both the main package (to `dist/`) and the WASM
package (to `wasm/dist/`).

### Building WASM locally

```bash
# Requires: wasm32-wasip1-threads Rust target
rustup target add wasm32-wasip1-threads
bun run build:wasm
# Place artifacts into sub-packages
bun x @napi-rs/cli artifacts
```

The `build:wasm` script uses `--platform` so the output is named
`fuzzy-search.wasm32-wasi.wasm` (matching the napi target
convention). `napi artifacts` then moves all generated files
into the correct `npm/` sub-packages automatically.

## Pull requests

- One logical change per PR.
- Include tests for bug fixes and new features.
- Run `bun run build:js && bun test && bun run test:node
&& bun run lint && bun run format`
  before submitting.
- Use [Conventional Commits](https://www.conventionalcommits.org/):
  `feat:`, `fix:`, `chore:`, `docs:`.
- Squash merge is enforced; keep the PR title clean.

If you change `package.json` version, also update the
generated metadata before opening a PR:

```bash
bun run sync:versions
bun run build
bun run check:metadata
```

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
