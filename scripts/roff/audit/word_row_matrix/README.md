# Word and aligned-row audit

Run from the repository root. The manifest freezes 335,908 logical
coordinates and retains every alias. Native collection follows sorted source
hashes in 128-source checkpoints. The binding includes the verified
reference, complete invocation recipes and the source manifest hash.
Completed chunks can be reused with the same binding.

```sh
audit=target/audits/word-row-matrix
python -m scripts.roff.audit.word_row_matrix "$audit" freeze
python -m scripts.roff.audit.word_row_matrix "$audit" oracle --calibration
python -m scripts.roff.audit.word_row_matrix "$audit" oracle --wall-seconds 1680 --artifact-mib 750
cargo build --locked --release -p mant-engine --example roff_word_row_profile
python -m scripts.roff.audit.word_row_matrix "$audit" product \
  --binary target/release/examples/roff_word_row_profile --follow
```

Use the same example source with the original codec to record the baseline
via `product --name baseline --binary ...`. The `compare` action pairs the
two observations by source hash and checkpoint and reports retained
passes, fixes, regressions, legacy differences and unresolved comparisons.
The profiler serializes each state through the public QueryBundle text
boundary, exports two independent Markdown chains and reimports each
chain twice. It records a document-validation verdict and a synthetic
SGR/plain comparison separately.

The direct comparison is restricted to the fixed MDOC prose core with
identifiable furniture. It preserves blank rows and authored spaces.
Unknown device projections and structurally richer sources retain raw
native and product observations with review status. Group witnesses list
all native alignment groups, including control-only groups and repeated
source coordinates. The recorder does not infer native cell lifetimes or
semantic owners from terminal strings. Its library observations do not
stand in for CLI or interactive TUI verification.

The calibration, cohort, full-native and product summaries report coverage
and budget stops independently. The local gate runs the small manifest and
comparator tests, while the dedicated fixtures test the confirmed behavior
through the actual rendering consumers.
