# Archived Rust formatter work in progress

Status: reference-only experiment, frozen on 2026-09-20. This is not an
accepted product fix or a release candidate. Do not merge this snapshot into
`dev` as a completed implementation.

- Original branch and baseline: `dev`,
  `aa211f7345dd80acd4b8b056e0deef0c368e18c6`.
- Archive branch: `codex/archive-dev-formatter-20260920`.
- Preserved payload: all 11 modified tracked source files and the previously
  untracked `crates/mant-codec/src/mandoc/inline/flow/line.rs`.
- The incomplete implementation introduces a Rust `FormatterMachine` while
  older boundary, no-break-field, indentation, and zero-advance state still
  participate in execution. Its implementation is not the new native
  structured-rendering baseline.
- The previous stop report recorded 333 codec tests passing and two failing;
  a 773-case CVS comparison improved 117 cases but regressed 156. These are
  historical results, not fresh validation of this archival commit.
- Archiving does not change source behavior, update behavioral expectations,
  or claim a successful test gate. No build or test suite was rerun merely
  to make the snapshot look complete.

The next design starts from the clean, committed `dev` baseline. Reuse source
analysis and independently verified regression inputs selectively; do not
carry over this unfinished Rust formatter state machine by default.

Recovery bundles, prior WIP backups, and performance evidence are retained
outside the checkout in
`/home/hby/dev/tmp/mant-roff-archive-20260920-VcXgBZ/`. Raw third-party manual
inputs and generated build products are not added to this archive branch.
