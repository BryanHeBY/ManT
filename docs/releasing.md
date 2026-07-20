# Release procedure

This guide is for maintainers. It describes the tagged-release automation and
does not form part of the everyday user installation path.

## Before tagging

1. Choose a semantic version and update both `package.json` and the
   `[workspace.package]` version in `native/Cargo.toml`.
2. Run the complete local verification boundary:

   ```sh
   bun run build
   ```

3. Commit the version change and ensure the main branch CI is green.

## Tag and draft release

The tag must exactly match the package and workspace version:

```sh
git tag vMAJOR.MINOR.PATCH
git push origin vMAJOR.MINOR.PATCH
```

The release workflow rebuilds and tests each supported Linux target on its
native GitHub runner. It packages `mant` and `mant-cli` together, uploads their
archives, assembles `SHA256SUMS`, and creates a **draft** GitHub Release with
generated notes. The draft is intentional: review the notes, archive names,
checksums, and licenses in GitHub before publishing it manually.

Linux x64 uses Bun's baseline target so the TUI does not require AVX2. macOS
continues to support local source builds, but public macOS archives stay
disabled until they can be Developer ID-signed and notarized for Gatekeeper.

## Repackaging locally

`bun run release:pack` packages already-tested artifacts; it never builds or
tests them. It validates the current host platform, package/workspace version
agreement, and optional release tag before writing the archive and its
individual SHA-256 checksum under `dist/`.

```sh
bun run build
MANT_RELEASE_TAG=vMAJOR.MINOR.PATCH bun run release:pack
```

Use this only to inspect a local archive. The tagged GitHub workflow remains
the public-release source of truth because it rebuilds on every target runner.
