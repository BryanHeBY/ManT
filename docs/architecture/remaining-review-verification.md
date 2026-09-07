# Remaining-review verification (2026-09-07)

Review input: `mant-review-remaining-2026-09-07.md`, baseline `9fb41349`.
Product/test/script changes were verified together at `03eea4ac`; the subsequent
documentation commit records these results without changing executable code.
IR convergence is outside this review.

## Disposition

- `be2976e5`: BSD/mandoc expansion now returns roots and diagnostics together,
  including macOS fragments and Linux fallback. Controlled-budget tests cover
  zero-match enumeration exhaustion, deep paths, skipped later roots, normal
  input, the 256-fragment limit and shared parent/fragment work.
- `d2db378e`: the `--offset` entry now specifies evidence class → document BFS →
  original IR order, while search remains BFS-first. Existing cross-document
  direct-entry-before-mention tests pass.
- `63bbcfc7`: activation rename/parent-sync failure points, failed rollback,
  backup recovery failure and idempotent retries execute against real trees.
  This is not a power-loss or exhaustive metadata-preparation simulation.
- `42f2c078`: real CLI SIGINT/SIGTERM, Rust UI callback panic and initialization
  EPIPE are checked in supervised PTYs. Assertions compare original termios
  and alternate-screen restoration. No production fault-injection flags added.
- `03eea4ac`: optional reference execution has bounded streams, deadlines,
  process-group cleanup, resource limits and fixed formatter/locale settings.
  This does not isolate filesystem/network access or deliberately detached
  processes; adversarial corpora still need external confinement.

## Local verification

Linux only; no new Windows/macOS native CI is claimed and no push was performed.
All compilation reused repository-local build directories. The existing
`check-packaged-crates.sh` forces a fresh `/tmp` build and was **not** run per
maintainer instruction; this is not an unqualified `scripts/check.sh` result.

- `cargo test --locked --workspace --all-features`: 1318 passed, 6 ignored.
- `cargo test --locked -p mant-engine --examples`: 31 passed.
- Workspace all-target/all-feature Clippy with `-D warnings`, rustdoc with
  `RUSTDOCFLAGS=-Dwarnings`, formatting, diff whitespace, installer/helper syntax,
  fuzz compilation and native symbol checks passed.
- `bash scripts/build-and-smoke.sh release`: build and smoke tests passed.
- Fidelity, structure, layout, projection, target and semantic self-checks plus
  audit coverage checks passed.
- Rebuilt the three IR profilers; fixture projection, target and semantic audit
  `--fixtures --recheck-recorded --verify --findings-only` each examined 37 pages:
  all clean. Target classification covered 14,985 owners with no target
  differences; semantic inspection covered 10,725 entries with no violations.
  Checked-in ledgers were not rewritten. No full-host corpus sweep is claimed.

## External-runner smoke evidence

Both `man` and `mandoc` ran a deterministic `--fixtures --max-pages 3` fidelity
sample through the new boundary. Each returned 2 clean, 1 review, 0 hard and
0 skipped. Inputs: `archlinux/archive_entry_stat.3`, `archlinux/bsdunzip.1`,
`bsd-closure/netbsd-drm.4`. The last page is the existing source-character
deviation: source `.An Jarom\('ir Dole\[vc]ek` becomes `Jaromír Doleček` in ManT,
while the reference drops the caron and emits `Doleek`. Source and both outputs
were inspected; this matches the existing confirmed-fixed fidelity ledger
entry for source SHA-256
`de68d037313c80fce7ecdb72da76e09e6009aafba7ad6673d1a6e7df7c6ed9a3`.
Runtime logs and isolated smoke ledgers are under `target/review-*`, not release
evidence for an entire corpus.
