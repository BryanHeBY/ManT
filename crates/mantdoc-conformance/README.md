# mantdoc-conformance

This unpublished crate owns the executable side of the versioned conformance
contract in [`tests/conformance/`](../../tests/conformance/).  It is excluded
from workspace default members and must never be published with `mantdoc`.

M1 establishes case identities, accepted-difference keys, and a checksum-pinned
upstream inventory. `mantdoc-corpus-inventory <mandoc-1.14.6.tar.gz>` verifies
the declared archive before listing the deterministic case-set hash; add a
corpus-local case ID to read exactly one re-verified parser input. Add
`--parse` after that ID to print the current native backend's bounded AST and
diagnostic counters for the same verified bytes. It never downloads, extracts,
or copies payload into the repository. The frozen migration evidence records a
complete 572/572 canonical-AST and `mant-ir` promotion before the temporary
oracle was retired. The maintained parser gate is now the native canonical
snapshot; the adjacent renderer differential remains 659 equal / 0 different /
0 errors against checksum-pinned upstream outputs. No difference is silently
accepted.
No test runner may silently update a source hash, golden, or accepted
difference.

M3 adds `--m3-execution`: it runs the reviewed roff execution gate declared in
[`m3-execution.toml`](../../tests/conformance/manifests/v1/m3-execution.toml).
The command re-verifies the archive and each source hash, then requires the
native raw-roff report's node count, expansion count, truncation state, and
complete diagnostic code/span sequence to equal the checked-in expectation. It
uses `Syntax::Roff`, insulating its counters from later man/mdoc structural
passes. `--m3-execution-report` is a maintainer-only inspected snapshot command
for an approved rebase; CI uses the asserting gate. This remains a narrow
execution gate, not a substitute for the M7 AST or renderer differential.

M4 adds `--m4-man-smoke`: it runs all 99 checksum-pinned stable man(7) inputs
through the native backend. Every report must be finite; 50 reviewed
source-malformed/recovery/style inputs have an exact typed-code sequence; the three
historical long-argument inputs are now parsed as valid physical continuations, and the documented
scope cases emit `man.unmatched-close` and/or `man.unclosed-block`. It is a
fast parser/recovery gate; full native canonical regression uses the separate
snapshot and renderer regression uses M9.

M5 adds `--m5-mdoc-smoke`: it runs all 276 checksum-pinned stable mdoc(7)
inputs through the native backend. Every report must be finite; 170 reviewed
scanner-, scope-, empty-display-, library-recovery-, tag-punctuation-, and link/description-recovery inputs have exact typed-code sequences. It freezes corpus
completion and recovery behavior while mdoc structural and validation coverage
grows; full native canonical regression uses the separate snapshot and
renderer regression uses M9.

For fast local native conformance work,
`python3 scripts/run-mantdoc-differential-shards.py /tmp/mandoc-1.14.6.tar.gz --shards 4`
builds once and runs independent M3–M6 gates, with deterministic M5
partitions. Add `--lanes m3,m4,m5,m6,m9` to include the parallel renderer
golden lane.

`mantdoc-canonical-snapshot <archive> --verify <snapshot>` is the separate
native regression gate. It parses each of the 572 stable inputs with `mantdoc`
alone, canonicalizes the report, and byte-compares the generated record set
with the checked-in snapshot. The snapshot carries the original oracle
identifier only as promotion provenance; this binary is pure Rust. It is
intentionally a release/M11 gate rather than part of the fast local loop:

```sh
cargo run --locked --package mantdoc-conformance --bin mantdoc-canonical-snapshot -- \
  /tmp/mandoc-1.14.6.tar.gz --verify \
  tests/conformance/baselines/v1/mandoc-1.14.6-native-canonical.sha256
```

M6 adds `--m6-preprocess-smoke`: it runs all 58 checksum-pinned stable tbl(7)
and eqn(7) inputs through the native backend. Every report must be finite; eight
reviewed recovery, post-table tab, and malformed-eqn inputs have exact typed-code
sequences.
This is a preprocessor smoke gate, not a
claim of full M6 grammar coverage; full native canonical regression uses the
separate snapshot and renderer regression uses M9.

Every backend run validates the decompressed source SHA-256 and a
cross-platform canonical parser-configuration fingerprint before parsing. A
case with changed bytes or a different syntax/limits/recovery configuration is
rejected rather than becoming an incomparable differential result.
