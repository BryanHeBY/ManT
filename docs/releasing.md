# Release procedure

This guide is for maintainers. Tagged automation builds reviewable draft
releases; publishing remains a deliberate human action.

Curated GitHub Releases are the canonical user-facing change history. Product
release notes group changes by outcome and include a **Breaking changes**
section with the old behavior, new behavior, and migration whenever users or
integrators must act.

## Before tagging

1. Version the seven crates independently in their own `Cargo.toml` files.
   A product release always bumps `crates/mant/Cargo.toml`; also bump every
   crate whose published source or public contract changed. Internal path
   dependencies use explicit caret requirements such as `^0.9.0`. Raise that
   minimum when a dependent requires a newer API, and bump the dependent crate
   because its published manifest changed. Refresh both `Cargo.lock` and the
   standalone `fuzz/Cargo.lock` before running locked checks.
   Move each compatibility-relevant item from the root `CHANGELOG.md`
   `Unreleased` section into a dated `<crate> X.Y.Z` entry for every selected
   package. Product release notes may summarize those entries by outcome, but
   must not replace crate-specific API or migration details.
   Compare each selected crate with its latest published tag before freezing
   versions. Use `cargo package --list -p <crate>` to identify the published
   source set, then inspect those paths with `git diff <previous-tag>..HEAD`.
   Any package-visible source, manifest, README, or generated documentation
   change requires a new crate version because crates.io cannot replace an
   existing immutable version. A clean package test proves the source set is
   buildable; it does not prove that its version is still publishable.
2. Regenerate and visually inspect the README screenshot:

   ```sh
   scripts/update-reader-screenshot.sh
   ```

   Commit the resulting `docs/assets/screenshots/mant-reader.png`. The script
   pins its font, geometry, terminal settings, and interaction sequence, but
   uses host-provided rendering tools. Compare captures with the same host
   toolchain where practical and always inspect the image before committing it.
3. Run the complete local verification boundary:

   ```sh
   bash scripts/check.sh
   cargo deny check
   ```

   If a deliberate protocol version change modified a generated structural
   schema, update every affected discriminator first and then regenerate its
   versioned snapshot with `scripts/update-protocol-schema-snapshot.sh`. Never
   refresh an existing-version snapshot merely to silence a compatibility
   failure. The `mant-protocol` crate version and native wire identifiers are
   separate: a crate-only implementation or documentation release can retain
   `mant.request/v0.9` and its related identifiers, while a breaking wire change
   must choose a new protocol family regardless of the crate's current semver.

4. Run a broad local roff fidelity audit before freezing the release. Use the
   syntax-priority, source-directed, and recorded-corpus workflows in the
   [development guide](development.md#roff-fidelity-audit), inspect every new
   candidate and hard failure, and promote each confirmed ManT defect to a
   licensed fixture with a focused Rust regression before tagging. Record the
   corpus identities, exact audit command, result summary, and reviewed ledger
   diff in the release preparation commit or pull request. This is required
   release evidence, but remains a maintainer-run discovery check rather than
   a host-dependent per-push CI gate.

5. Inspect the publishable file list for all seven crates. Each package must
   contain its applicable complete license texts and no unexpected fixture or
   documentation assets. The canonical `scripts/check.sh` path also packages
   all seven crates and tests their exact source sets in an isolated temporary
   workspace, so repository-only fixtures cannot silently break downstream
   packager tests:

   ```sh
   for package in mant-ir mant-protocol libmandoc-rs mant-sources mant-engine mant-ui mant; do
     cargo package --locked --list -p "$package"
   done
   ```

6. Commit every release change, sync that exact commit to `main`, and ensure
   the main branch CI is green. The recommended installers and agent prompt
   read their scripts and documentation from `main`, so do not publish a tag
   that is ahead of the default branch.

## Configure crates.io publication

The crates.io packages form one dependency graph. `mant-ui` first entered
crates.io as the `0.4.1` bootstrap release against the existing `0.4.0`
contracts. Starting with `0.5.0`, the original five packages use one lockstep
version; `mant-sources` joined the same graph in `0.6.0`. Version `0.7.0`
replaces the mixed `mant-ast` package with separate `mant-ir` and
`mant-protocol` packages, and renames `mant-core` to `mant-engine`, under the
same lockstep policy. Version `0.9.0` is the final lockstep release; later
development assigns each crate its own semver while retaining this publication
order:

```text
mant-ir ─> mant-protocol
mant-ir + mant-protocol + mant-sources + libmandoc-rs ─> mant-engine
mant-ir + mant-protocol + mant-engine (dev) ─> mant-ui
mant-ir + mant-protocol + mant-sources + mant-engine + mant-ui ─> mant
```

Here an arrow means the package on the left must be visible in crates.io before
the package on the right is validated. The `mant-engine` edge into `mant-ui` is
a development dependency used by doctests and integration tests, not a runtime
frontend dependency. `scripts/publish-crates.sh` encodes the complete linear
order: `mant-ir`, `mant-protocol`, `libmandoc-rs`, `mant-sources`,
`mant-engine`, `mant-ui`, then `mant`.

Each previously published package must configure the same crates.io Trusted
Publisher:

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

On a product tag push, `scripts/publish-crates.sh` inspects the version of each
crate, packages every version not already present, and publishes in dependency
order. A package tag selects only that package. Each selected package is
validated immediately before upload, and the script waits for its own version
to reach the index before continuing. Existing immutable versions are skipped,
making a partially completed job safe to rerun. Installing `mant` installs the
reader, structured CLI, and MCP server as one executable. `mant-ui` is a
reusable library crate and does not install a second command.

An internal requirement like `^0.9.0` accepts compatible `0.9.x` releases but
not `0.10.0`. If a lower-level crate makes a breaking pre-1.0 change, bump its
minor version, update and bump every dependent that adopts it, then publish
bottom-up. If a dependent starts using an API introduced in `0.9.2`, change its
minimum to `^0.9.2` and bump that dependent even when no Rust source changed.
Because `libmandoc-rs` declares `links = "libmandoc_rs"`, Cargo permits only
one version of that native-link crate in a dependency graph; its breaking
minor upgrades therefore require coordinated dependent releases even when
other pure-Rust crates could otherwise coexist at multiple versions.
Do not publish changed crate source under an existing version: the native
product artifacts are built from workspace paths, while Cargo installations
resolve the published crates, so failing to bump a changed package could make
the two distributions differ.

The post-0.9.0 decoupling changed the four dependent package manifests from
exact internal requirements to explicit caret requirements; the three
dependency roots have no internal edge to rewrite. The first independent
version baseline is prepared as `0.9.1` across all seven crates so every
package-visible post-0.9.0 change has a publishable identity. Later releases
may bump only the crates that changed. This one-time transition is a
packaging-contract change, not a return to permanent lockstep versioning.

Never move a tag after crates.io publication. Registry versions are immutable.

## Publish one crate

A crate-only release uses `<package>-vMAJOR.MINOR.PATCH`, for example:

```sh
git tag mant-ir-v0.9.1
git push origin mant-ir-v0.9.1
```

Valid package prefixes are `mant-ir`, `mant-protocol`, `libmandoc-rs`,
`mant-sources`, `mant-engine`, and `mant-ui`. The tag version must exactly match
that package's manifest. The same `release.yml` workflow verifies full CI for
the tagged SHA, skips native product archives and the GitHub Release, pauses at
the protected `crates-io` Environment, and publishes only the tagged package.
All of its declared dependency versions must already be visible on crates.io.
Sync the exact green release commit to `main` before tagging, as with any other
public release checkpoint.

The `mant` crate has an independent manifest version but no `mant-v*` crate-only
tag. Its cargo-binstall metadata points at matching native GitHub assets, so it
must be published by the product `vMAJOR.MINOR.PATCH` workflow that builds and
attests those assets.

Package tags are optional when a new crate version ships as part of a product
release: the product `vMAJOR.MINOR.PATCH` tag already anchors every unpublished
crate version selected from that commit. Use a package tag when publishing a
library fix independently of a new `mant` binary release.

### One-time new-crate bootstrap for `0.7.0`

The `mant-ir`, `mant-protocol`, and `mant-engine` names first enter crates.io in
`0.7.0`. Trusted Publishing cannot create a crate, so each required a one-time
manual upload from the frozen, green release commit before its Trusted
Publisher could be configured. `mant-ir` and `mant-protocol` could be
bootstrapped before the tag; wait for `mant-ir` to become visible before
packaging its exact-version dependent:

```sh
cargo login
cargo publish --locked -p mant-ir
cargo info mant-ir@0.7.0
cargo publish --locked -p mant-protocol
cargo info mant-protocol@0.7.0
```

`mant-engine` also depends on the exact `0.7.0` releases of `libmandoc-rs` and
`mant-sources`. The initial tag job can publish those established crates and
then stop when crates.io rejects creation of `mant-engine`. Once both
dependencies are visible, bootstrap the remaining new name from the same
release commit:

```sh
cargo publish --locked -p mant-engine
cargo info mant-engine@0.7.0
cargo logout
```

Use a temporary narrowly scoped token for these manual uploads. Configure the
same Trusted Publisher for all three new packages after their initial upload,
then rerun the failed tag job. The publish script skips every immutable version
already present and continues with `mant-ui` and `mant`. Do not repeat this
bootstrap after `0.7.0`.

## Tag and draft release

The product tag must exactly match the version in `crates/mant/Cargo.toml`.
Create it from the clean `main` release commit:

```sh
git tag vMAJOR.MINOR.PATCH
git push origin vMAJOR.MINOR.PATCH
```

The release workflow first requires a completed full CI suite for the exact
tagged commit. It then builds and smoke-tests each optimized Linux and Windows
binary on its native GitHub runner without repeating the already recorded test,
Clippy, MSRV, coverage, and supply-chain jobs. After all targets pass, it
creates a **draft** GitHub Release and independently pauses at the protected
`crates-io` Environment before publishing crates. Review the tag and draft
artifacts, replace the generated notes with a curated user-facing summary,
then approve that deployment. Wait for every newly selected crate version to
become visible before making the GitHub Release public; the macOS installer
depends on the published `mant` crate. A manually dispatched product release
rebuilds a named tag and draft but publishes no crates by default and can create
the draft only when that tag has no existing GitHub Release. A manually
dispatched package tag likewise publishes nothing unless `publish_crates` is
enabled. For a failed tag workflow that published no crate, enable that input;
the protected `crates-io` Environment still requires approval. Leave it
disabled for artifact-only product rebuilds. If publication partially
succeeded, rerun the original failed job so the release script can detect and
skip versions already present on crates.io. Manual retries always use the
immutable tag's product tree while taking the release helpers from the trusted
workflow revision on `main`. Automation fixes can therefore recover older tags
without changing their product input.

Each native archive keeps the versioned `manuals/` set beside the executable,
and the release also publishes the same documents as the platform-independent
`mant-MAJOR.MINOR.PATCH-manuals.tar.gz` asset. Both forms are checksummed and
attested. User-facing release notes should lead with
the one-line installers, which register these documents automatically. Manual
archive users can copy `manuals/*.md` into
`${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents`; a system package may
ship them as ordinary package documentation and explain how to register them.
The documents are optional at runtime, but user-scoped copies make the command,
IR, protocol, and input references available to CLI and MCP discovery without
a repository checkout.

Linux x64 uses the baseline target so the executable does not require AVX2.
Windows x64 is distributed as a ZIP and contains bundled libmandoc alongside
Markdown support. Its `manuals\*.md` files can be installed below
`%APPDATA%\ManT\documents`.
macOS supports Cargo installation and local source builds, but public macOS
archives remain disabled until they can be Developer ID-signed and notarized.

The `mant` crate's cargo-binstall metadata maps the three public Rust targets
to these human-readable archive names and their nested executable paths.
`crates/mant/tests/release_metadata.rs` keeps that mapping synchronized with
the packaging and one-line installer scripts. The installers resolve GitHub's
latest public release, download `SHA256SUMS`, install the platform archive and
register its bundled manuals. They keep a versioned receipt so later runs can
update the same destinations or safely uninstall only installer-owned files.
If an archive name, root, checksum publication, or receipt schema changes,
update the crate metadata, installers, installation guide, and regression test
in the same commit. cargo-binstall and the one-line installers can download an
archive only after the draft GitHub Release becomes public; before that,
cargo-binstall may fall back to a source build.

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

The platform-independent manual archive can be reproduced without first
building a binary:

```sh
MANT_RELEASE_TAG=vMAJOR.MINOR.PATCH bash scripts/package-manuals.sh
```

The equivalent Windows commands for a native package are:

```powershell
.\scripts\check-windows.ps1
$env:MANT_RELEASE_TAG = "vMAJOR.MINOR.PATCH"
.\scripts\package-release.ps1
```

Before publishing, regenerate `THIRD_PARTY_LICENSES.html` with cargo-about
0.9.2 and confirm there is no diff. Then inspect that the executable,
self-hosted document, project license, generated Rust dependency report,
parser third-party notice, upstream inventory, and complete reusable license
texts are present:

```sh
tar -tzf dist/mant-MAJOR.MINOR.PATCH-linux-ARCH.tar.gz
```

On Windows, list the ZIP with the bundled `tar.exe`:

```powershell
tar.exe -tf dist\mant-MAJOR.MINOR.PATCH-windows-x64.zip
```

The tagged GitHub workflow remains the public-release source of truth because
it performs a clean optimized build on every target runner after verifying the
tagged source passed full CI. It generates a target-specific CycloneDX SBOM,
publishes it beside the archives, creates provenance attestations for the
native and manual release files, and publishes their Sigstore bundles beside
the archives. It also cryptographically binds each native archive to its SBOM.
GitHub also stores these attestations in its attestations API; the attached
`*.sigstore.json` copies make the signatures portable and discoverable by
release scanners. After
downloading a release asset, verify that it was produced by this repository:

```sh
gh attestation verify mant-MAJOR.MINOR.PATCH-PLATFORM.EXT \
  --repo BryanHeBY/ManT
```

To inspect the SBOM attestation as well as the default provenance statement,
select the CycloneDX predicate:

```sh
gh attestation verify mant-MAJOR.MINOR.PATCH-PLATFORM.EXT \
  --repo BryanHeBY/ManT \
  --predicate-type https://cyclonedx.org/bom
```

Releases created before tagged build attestations were enabled can be signed
with the manually dispatched `Attest Existing Release` workflow. That workflow
downloads the existing archives, signs a custom maintainer-endorsement
statement with GitHub OIDC, records it in the attestations API, and attaches a
portable `*.sigstore.json` bundle to the release. It deliberately states that
the endorsement is not provenance from the original build; never represent a
retrospective signature as historical build provenance.
