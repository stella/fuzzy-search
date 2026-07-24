# Changesets

Every pull request that changes the published runtime must include a changeset:

```sh
bun run changeset
```

Choose `patch`, `minor`, or `major` and write a concise user-facing summary. If
a source change intentionally needs no release, record that decision with
`bun run changeset --empty`.

Changesets accumulate on `main`. The release PR workflow consumes them, updates
the changelog, and synchronizes the selected version into `VERSION`, every npm
package, Cargo metadata, and the generated native loader guard. Merging that
version PR triggers `.github/workflows/release.yml`; Changesets never publishes
packages.
