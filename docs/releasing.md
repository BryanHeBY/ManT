# Release procedure

This guide is for maintainers. It describes the tagged-release automation and
does not form part of the everyday user installation path.

## Before tagging

1. Choose a semantic version and update `package.json`,
   `apps/mantui/package.json`, and the `[workspace.package]` version in
   `engine/Cargo.toml`. The four published Rust crates use one lockstep
   version; update the exact internal dependency requirements in
   `engine/crates/mant-core/Cargo.toml` and
   `engine/crates/mant/Cargo.toml` at the same time.
2. Run the complete local verification boundary:

   ```sh
   bun run build
   ```

3. Commit the version change and ensure the main branch CI is green.

## Publish the Rust crates

The crates.io packages form one dependency chain and must be published in
this order:

```text
libmandoc-rs ─┐
              ├─> mant-core ─> mant
mant-ast ─────┘
```

For the first release, authenticate locally with `cargo login`, review each
package, and publish it before moving to the next dependent package:

```sh
cd engine

cargo publish --dry-run --locked -p libmandoc-rs
cargo publish --locked -p libmandoc-rs

cargo publish --dry-run --locked -p mant-ast
cargo publish --locked -p mant-ast

cargo publish --dry-run --locked -p mant-core
cargo publish --locked -p mant-core

cargo publish --dry-run --locked -p mant
cargo publish --locked -p mant
```

Wait for each new package to become visible in the crates.io index before
checking or publishing its dependent package. The `mant` package installs the
native CLI and MCP server; `mantui` remains part of the GitHub release archive
and is not installed by Cargo.

After all four package names exist, configure the same crates.io Trusted
Publisher for each package. Subsequent releases can exchange GitHub's OIDC
identity for short-lived publishing credentials instead of storing a Cargo
token in repository secrets. Keep that publishing workflow gated on the
manually reviewed GitHub Release becoming public, rather than on creation of
the draft.

## Tag and draft release

The tag must exactly match the package and workspace version:

```sh
git tag vMAJOR.MINOR.PATCH
git push origin vMAJOR.MINOR.PATCH
```

The release workflow rebuilds and tests each supported Linux target on its
native GitHub runner. It packages `mantui` and `mant` together, uploads their
archives, includes their self-hosted `mant.md` and `mantui.md` manuals,
assembles `SHA256SUMS`, and creates a **draft** GitHub Release with generated
notes. The draft is intentional: review the notes, archive names, checksums,
manuals, and licenses in GitHub before publishing it manually.

Linux x64 uses Bun's baseline target so the TUI does not require AVX2. macOS
continues to support local source builds, but public macOS archives stay
disabled until they can be Developer ID-signed and notarized for Gatekeeper.

## Repackaging locally

`bun run release:pack` packages already-tested artifacts; it never builds or
tests them. It validates the current host platform, agreement among the root,
mantui, and Rust workspace versions, and the optional release tag before
writing the archive and its individual SHA-256 checksum under `dist/`.

```sh
bun run build
MANT_RELEASE_TAG=vMAJOR.MINOR.PATCH bun run release:pack
```

Use this only to inspect a local archive. The tagged GitHub workflow remains
the public-release source of truth because it rebuilds on every target runner.
