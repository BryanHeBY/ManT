# Release procedure

This guide is for maintainers. It describes the tagged-release automation and
does not form part of the everyday user installation path.

## Before tagging

1. Choose a semantic version and update `package.json` and the
   `[workspace.package]` version in `engine/Cargo.toml`. The five published Rust
   crates use one lockstep version; update every exact internal dependency in
   their `Cargo.toml` files at the same time.
2. Run the complete local verification boundary:

   ```sh
   bun run build
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

Authenticate locally with `cargo login`, review each package, and publish it
before moving to a dependent package:

```sh
cd engine

for package in libmandoc-rs mant-ast mant-core mant-ui mant; do
  cargo publish --dry-run --locked -p "$package"
  cargo publish --locked -p "$package"
done
```

The loop documents order, not registry timing: wait for each new package to
become visible in the crates.io index before checking or publishing its
dependent package. Installing `mant` installs the complete interactive reader,
structured CLI, and MCP server as one executable. `mant-ui` is a reusable
internal frontend crate; it does not install a second command.

Configure the same crates.io Trusted Publisher for each package after its
first release. Subsequent releases can exchange GitHub's OIDC identity for
short-lived publishing credentials instead of storing a Cargo token in
repository secrets. Keep publishing gated on the manually reviewed GitHub
Release becoming public, rather than on creation of the draft.

## Tag and draft release

The tag must exactly match the package and workspace version:

```sh
git tag vMAJOR.MINOR.PATCH
git push origin vMAJOR.MINOR.PATCH
```

The release workflow rebuilds and tests each supported Linux target on its
native GitHub runner. It packages one `mant` executable, includes the
self-hosted `mant.md` manual, assembles `SHA256SUMS`, and creates a **draft**
GitHub Release with generated notes. Review the notes, archive names,
checksums, manual, and licenses in GitHub before publishing it manually.

Linux x64 uses the baseline build target so the executable does not require
AVX2. macOS continues to support Cargo installation and local source builds,
but public macOS archives stay disabled until they can be Developer ID-signed
and notarized for Gatekeeper.

## Repackaging locally

`bun run release:pack` packages an already-tested `dist/mant`; it never builds
or tests it. It validates the current host platform, agreement between the
root and Rust workspace versions, and the optional release tag before writing
the archive and its individual SHA-256 checksum under `dist/`.

```sh
bun run build
MANT_RELEASE_TAG=vMAJOR.MINOR.PATCH bun run release:pack
```

Use this only to inspect a local archive. The tagged GitHub workflow remains
the public-release source of truth because it rebuilds on every target runner.
