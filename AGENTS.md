# Repository agent rules

## Roff and mandoc compatibility

- Treat `crates/libmandoc-rs/vendor/mandoc-cvs-20260927T130954Z` as the pinned primary
  behavioral and implementation reference. Groff is a secondary comparison;
  do not replace the selected CVS contract with guessed behavior.
- Whenever a regression, behavioral discrepancy, or review finding involves
  roff parsing, formatting, rendering, or lowering, **before changing any
  product code**, locate and read the corresponding execution path in the
  pinned CVS source. This applies again to every newly discovered adjacent or
  follow-up regression, even when it appears to share an already investigated
  root cause. Do not implement a fix from observed output alone.
- Before adding or changing a behavioral regression test, run the exact
  minimal roff input with `target/mandoc-migration/reference/mandoc` and derive
  the expected result from that run. The reference run must precede writing the
  assertion. Record the relevant CVS source path or execution rule in the test
  comment when the behavior is non-obvious.
- If the fixed CVS reference binary is unavailable, stop the behavioral edit
  instead of inventing an expectation. Rebuild or restore the reference first.
- Prefer fixes at the shared execution/state boundary identified in upstream
  source. Do not add document-specific rules or isolated output patches when
  the upstream cause is a formatter, parser, or macro execution rule.

## Native probing and the reference binary

- Never build, patch, or run `configure`/`make` inside
  `crates/libmandoc-rs/vendor/`. The vendor tree stays exactly as
  committed; copy it to a scratch directory for gdb probes or
  instrumented builds.
- The behavioral reference lives at
  `target/mandoc-migration/reference/mandoc` and dies with every
  `cargo clean`. Restore it with `scripts/rebuild_reference_mandoc.sh`
  (recipe verified byte-identical on the 54-case matrix) instead of
  improvising a rebuild.
