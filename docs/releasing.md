# Release procedure

This guide is for maintainers. Tagged automation builds reviewable draft
releases; publishing remains a deliberate human action.

## Before tagging

1. Choose a semantic version and update `[workspace.package].version` in
   `Cargo.toml`. The five published Rust crates use one lockstep version, so
   update every exact internal dependency in their manifests at the same time.
2. Run the complete local verification boundary:

   ```sh
   bash scripts/check.sh
   ```

3. Commit the version change and ensure the main branch CI is green.

## Publish the Rust crates

The crates.io packages form one dependency graph and must be published from
the leaves toward the unified command:

```text
libmandoc-rs ─┐
              ├─> mant-core ─┬─> mant-ui ─> mant
mant-ast ─────┘               └────────────> mant
```

Authenticate with `cargo login`, review each package, and publish it before
moving to a dependent package:

```sh
for package in libmandoc-rs mant-ast mant-core mant-ui mant; do
  cargo publish --dry-run --locked -p "$package"
  cargo publish --locked -p "$package"
done
```

Wait for each package to appear in the crates.io index before publishing its
dependent package. Installing `mant` installs the reader, structured CLI, and
MCP server as one executable. `mant-ui` is a reusable library crate and does
not install a second command.

Configure crates.io Trusted Publishing for later releases so GitHub Actions
can exchange its OIDC identity for short-lived credentials. Do not store a
long-lived Cargo token in the repository.

## Tag and draft release

The tag must exactly match the Cargo workspace version:

```sh
git tag vMAJOR.MINOR.PATCH
git push origin vMAJOR.MINOR.PATCH
```

The release workflow rebuilds and tests each supported Linux target on its
native GitHub runner. It packages one `mant` executable, includes the
self-hosted `mant.md` manual, assembles `SHA256SUMS`, and creates a **draft**
GitHub Release with generated notes. Review the notes, archive names,
checksums, manual, and licenses before publishing the draft.

The archive keeps `mant.md` beside the executable so installation remains
transparent. User-facing release notes should recommend copying it to
`${XDG_DATA_HOME:-$HOME/.local/share}/mant/mant.md`; a system package should
install it as `/usr/local/share/mant/mant.md` (or the matching prefix-relative
`share/mant/mant.md`). The document is optional at
runtime, but installing it makes `mant mant` and MCP document discovery work
without a repository checkout.

Linux x64 uses the baseline target so the executable does not require AVX2.
macOS supports Cargo installation and local source builds, but public macOS
archives remain disabled until they can be Developer ID-signed and notarized.

## Inspect an archive locally

Packaging never builds or tests. It validates the Cargo version, optional tag,
and native Linux architecture, then reproducibly archives the already-built
executable:

```sh
bash scripts/check.sh
MANT_RELEASE_TAG=vMAJOR.MINOR.PATCH bash scripts/package-release.sh
```

Set `MANT_RELEASE_TARGET=linux-x64` or `linux-arm64` to assert the expected
runner identity. `MANT_BINARY` may point at another already-built executable.
Archives and individual SHA-256 files are written under `dist/`.

Before publishing, inspect that both the executable and self-hosted document are
present:

```sh
tar -tzf dist/mant-MAJOR.MINOR.PATCH-linux-ARCH.tar.gz
```

The tagged GitHub workflow remains the public-release source of truth because
it rebuilds on every target runner.
