# Pristine mandoc oracle identities

The files in this directory define the maintainer-only trust boundary for
independent mandoc behavior checks. Ordinary Cargo builds and unit tests do
not download, build, or execute an external oracle.

`recipe.json` is the closed build recipe. `registry.json` explicitly lists
accepted platform-specific attestations; discovering a new binary hash never
updates that registry automatically. Each attestation binds the checked-in
`SOURCE` and `FILES`, a pristine archive, the recipe, configure evidence,
toolchain/platform identity, the resulting binary hash, and supported output
profiles.

Build a candidate below `target/`:

```sh
crates/libmandoc-rs/scripts/build-oracle \
  --archive target/mandoc-migration/<snapshot>/upstream-<snapshot>.tar.gz \
  --output target/mandoc-migration/oracle-<snapshot>-<platform> \
  --identity <snapshot>-<platform-toolchain>
```

Review the generated `BUILD.json` and `attestation.json` before copying the
attestation into `attestations/` and registering its exact hash. The build
reads the raw archive directly and never applies `patches/` or reads the
patched vendor tree.

Every mandoc-backed audit requires an explicit registered attestation and
archive. Verify one without running an audit:

```sh
scripts/mandoc-oracle-preflight \
  --attestation crates/libmandoc-rs/upstream/oracle/attestations/<identity>.json \
  --binary target/mandoc-migration/reference/mandoc \
  --archive target/mandoc-migration/<snapshot>/upstream-<snapshot>.tar.gz \
  --identity <identity> \
  --profile utf8
```

The preflight rejects an unregistered or moved attestation, another snapshot,
source/recipe/config drift, a swapped binary, a wrong archive, an unsupported
profile, and a platform mismatch. Audits repeat it after execution so drift
cannot produce a clean result.

`target/mandoc-migration/reference/mandoc` is the required active reference
entry point. Copy the newly registered pristine binary there and preflight
that exact path after each source refresh. Keep older binaries under their
separate build directories; their registry entries remain historical.

## Restore after cleaning build outputs

```sh
scripts/rebuild_reference_mandoc.sh
```

This command selects the active registered identity for the current source
lock and platform. It checks the archive, recipe, build evidence and binary
for all three profiles, including when the reference already exists. A
matching retained build can restore the entry point without recompiling.
Otherwise it invokes `build-oracle` and accepts only the exact registered
attestation; it never registers a different build automatically.

When the archive was also removed, pass a saved exact archive with
`--archive /path/to/upstream-cvs-20260927T130954Z.tar.gz`, or provide a CVS
client with `--cvs /path/to/cvs` (also accepted through `CVS`). The latter
uses `freeze-cvs-snapshot` to perform two official checkouts at the locked
UTC cutoff and verifies the complete archive and source-lock hashes. All
checkouts and compilation remain under `target/`; no vendor patch, configure
operation, output shim or archive reconstruction from vendor is involved.

## Reproducible identity migration on 2026-09-30

The prior standalone restoration used patched vendor sources and a UTF-8
hook that passed Unicode codepoints to `putchar()`. That binary was neither
the registered artifact nor a pristine oracle. It has been replaced with
a formal pristine build from the existing locked archive, SHA-256
`b1679b38a1afde6764e48a0c9682921aaf8cd746a1f150aeb75ffad0f625b7e2`.
The source snapshot and `SOURCE`, `FILES`, and `CVS_INVENTORY.json` hashes
are unchanged.

The previous formal recipe also embedded a random temporary compilation
directory in DWARF, preventing its artifact identity from being reproduced
after `cargo clean`. The recipe now retains the configured compiler flags
and adds `-ffile-prefix-map={source}=/mandoc-pristine` at the build step.
`{source}` denotes the actual pristine compilation directory; the build
record and attestation retain this template so temporary names cannot change
their hashes. Configuration and upstream source are unchanged.

Two independent builds at separate output paths produced identical binaries,
build records and all four configure evidence files. The new active identity
is `cvs-20260927T130954Z-linux-x86_64-gcc-16.2.1-reproducible-20260930`,
with binary SHA-256
`482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6`.
The previous identity remains historical. The reference passes ASCII, UTF-8
and HTML preflight; UTF-8 probes preserve both `α` and `中` as valid UTF-8.
An isolated copy with no archive, binary or build evidence restored the same
identity through the public restoration script using the saved exact
archive. A different toolchain or configure result fails restoration and
requires a separately reviewed maintainer attestation.
