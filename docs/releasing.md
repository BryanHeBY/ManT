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

## Configure crates.io publication

The crates.io packages form one dependency graph. `mant-ui` first entered
crates.io as the `0.4.1` bootstrap release against the existing `0.4.0`
contracts; later workspace releases keep all five packages on one version:

```text
libmandoc-rs ─┐
              ├─> mant-core ─┬─> mant-ui ─> mant
mant-ast ─────┘               └────────────> mant
```

Each package must configure the same crates.io Trusted Publisher:

```text
repository owner: BryanHeBY
repository:       ManT
workflow:         release.yml
environment:      crates-io
```

The GitHub repository has a matching `crates-io` Environment. It should require
a maintainer review, permit self-review for this personal repository, reject
non-release refs, and contain no long-lived Cargo token. The release job has
only `contents: read` and `id-token: write`; the official crates.io action
exchanges that identity for a short-lived credential.

On a tag push, `scripts/publish-crates.sh` validates all package archives and
publishes `mant-ast`, `libmandoc-rs`, `mant-core`, `mant-ui`, and `mant` in
dependency order. It waits for each exact version to reach the registry before
continuing and skips versions already present, so a partially completed job is
safe to rerun. Installing `mant` installs the reader, structured CLI, and MCP
server as one executable. `mant-ui` is a reusable library crate and does not
install a second command.

Never move a tag after crates.io publication. Registry versions are immutable.

## Tag and draft release

The tag must exactly match the Cargo workspace version:

```sh
git tag vMAJOR.MINOR.PATCH
git push origin vMAJOR.MINOR.PATCH
```

The release workflow rebuilds and tests each supported Linux and Windows target
on its native GitHub runner. After all targets pass, it creates a **draft**
GitHub Release and independently pauses at the protected `crates-io`
Environment before publishing crates. Review the tag and draft artifacts, then
approve that deployment. A manually dispatched release rebuilds a named tag
and draft but deliberately does not publish crates; rerun the original tag
workflow if crates.io publication needs retrying.

The archive keeps `mant.md` beside the executable so installation remains
transparent. User-facing release notes should recommend copying it to
`${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents/mant.md`; a system package
should install it as `/usr/local/share/mant/documents/mant.md` (or the matching
prefix-relative `share/mant/documents/mant.md`). The document is optional at
runtime, but installing it makes `mant mant` and MCP document discovery work
without a repository checkout.

Linux x64 uses the baseline target so the executable does not require AVX2.
Windows x64 is distributed as a ZIP and contains the Markdown-native product;
it does not include libmandoc or Unix manual support. The bundled `mant.md` can
be installed at `%APPDATA%\ManT\documents\mant.md`.
macOS supports Cargo installation and local source builds, but public macOS
archives remain disabled until they can be Developer ID-signed and notarized.

The `mant` crate's cargo-binstall metadata maps the three public Rust targets
to these human-readable archive names and their nested executable paths.
`crates/mant/tests/release_metadata.rs` keeps that mapping synchronized with
both packaging scripts. If an archive name or root changes, update the crate
metadata and regression test in the same commit. cargo-binstall can download
an archive only after the draft GitHub Release becomes public; before that it
may fall back to a source build.

## Inspect an archive locally

Packaging never builds or tests. It validates the Cargo version, optional tag,
and native platform identity, then archives the already-built executable:

```sh
bash scripts/check.sh
MANT_RELEASE_TAG=vMAJOR.MINOR.PATCH bash scripts/package-release.sh
```

Set `MANT_RELEASE_TARGET=linux-x64` or `linux-arm64` to assert the expected
runner identity. `MANT_BINARY` may point at another already-built executable.
Archives and individual SHA-256 files are written under `dist/`.

The equivalent Windows commands are:

```powershell
.\scripts\check-windows.ps1
$env:MANT_RELEASE_TAG = "vMAJOR.MINOR.PATCH"
.\scripts\package-release.ps1
```

Before publishing, inspect that both the executable and self-hosted document are
present:

```sh
tar -tzf dist/mant-MAJOR.MINOR.PATCH-linux-ARCH.tar.gz
```

The tagged GitHub workflow remains the public-release source of truth because
it rebuilds on every target runner.
