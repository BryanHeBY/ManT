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
  --binary target/mandoc-migration/oracle-<snapshot>-<platform>/mandoc \
  --archive target/mandoc-migration/<snapshot>/upstream-<snapshot>.tar.gz \
  --identity <identity> \
  --profile utf8
```

The preflight rejects an unregistered or moved attestation, another snapshot,
source/recipe/config drift, a swapped binary, a wrong archive, an unsupported
profile, and a platform mismatch. Audits repeat it after execution so drift
cannot produce a clean result.
