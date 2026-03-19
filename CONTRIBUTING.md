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
