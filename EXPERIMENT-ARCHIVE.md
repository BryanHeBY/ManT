# Archived native execution-report experiment

Status: reference-only experiment, frozen on 2026-09-20. This snapshot includes
unfinished K23 work. It is not an accepted production switch or a release
candidate, and must not be merged into `dev` as a completed implementation.

- Archive branch: `codex/roff-execution-rewrite`.
- Last committed implementation checkpoint before this snapshot:
  `f7d6f201e1502be532e83eed68c6f493dd47e69a` (K22).
- Preserved payload: all 52 modified tracked files and all 27 previously
  untracked source, patch, documentation, and fixture files.
- K23 still has unclosed real-page behavior and performance issues. K24 was
  not started and the legacy implementation was not removed.
- The frozen performance report measured GCC release loading at roughly
  3100 ms and 1017 MiB peak RSS, versus roughly 268 ms and 74.5 MiB at K22.
  These are prior measurements, not a new benchmark of the archival commit.
- The prior closeout reported passing libmandoc-rs and mant-codec tests but
  did not rerun the full workspace, real fixture set, vendor replay, or all
  product acceptance gates. Archival does not upgrade that evidence.
- No source behavior or behavioral assertions were changed to produce this
  snapshot. No build or test suite was rerun as an archival prerequisite.

The revised direction is a compact native structured-rendering output, not
the full execution-report and responsive-geometry reconstruction pipeline.
Existing safety work, native lifecycle knowledge, provenance techniques, and
CVS regression assets remain useful references. Migrate them selectively,
with independent correctness and performance validation.

Recovery bundles, the frozen performance report and evidence, previous WIP
backups, and non-build artifacts from this worktree's `target` directory are
retained outside the checkout in
`/home/hby/dev/tmp/mant-roff-archive-20260920-VcXgBZ/`. Generated build products
are disposable and are not part of this source archive.
