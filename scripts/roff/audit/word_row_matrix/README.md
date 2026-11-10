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
Both sides must bind the same frozen manifest, native evidence and saved
verdict contract. An archived pair with the same recorded contract remains
comparable; a new verdict must not be compared to an old one. To change
classification rules, apply them to both sets of raw observations or
capture both sides again. Do not edit binding metadata to bypass the check.
The comparison report records that saved verdict contract and the live
comparison contract separately. Markdown readback rules belong to the
latter because they are derived from raw observations during comparison.
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
and budget stops independently. The local gate discovers the manifest and
comparator tests under `scripts/roff/tests`, while the dedicated fixtures
test confirmed behavior through the actual rendering consumers. Each lane
declares its CLI flag, frozen inputs and specialized verdict in `lanes.py`.

The horizontal-rewind and literal-owner follow-up is frozen independently
with `freeze --followup`: 54,260 logical coordinates, including degenerate
literal insertions, the ten source adapters, six field adapters, and
bounded long prefixes. Keep its oracle and product checkpoints separate
from the original manifest; hash is the join key across both manifests.

Use `freeze --pending-rows` for the pending section handoff audit. This
freezes 648 source and logical coordinates (432 marker controls, 144
adjacent `.br` controls, and 72 `.br` followed by `\&`). `nativePendingRow`
records authored and executed `.br` separately, exact row intervals, and
the glyphs and source positions retained by the old section, new heading,
and new body. Native rows shared across section owners use the explicit
`allowed-owner-split` status; unknown row forms remain Review. Run the
same oracle/product/compare commands against a separate directory.

Use `freeze --section-tail` to replay the ordered tail-pass follow-up
separately. Its 600 sources cover 512 core coordinates, 24 styled and link
wrappers, 32 HEAD variants, 16 boundary controls, 12 chained passes, and
four horizontal-rewind controls. The native reader comparison preserves
middle and trailing rows and separately certifies source ownership, heading
identity, JSON/ANSI transport, and representative font/link wrappers.
It admits only source-proven section splits, retained authored spaces, and
the documented single initial literal-row policy. A removed adjacent
`.br` and a retained `.br` have distinct tree evidence; a same-owner
control has no new section. Markdown stages retain their own readback axes.

Use `freeze --section-padding` for the completed-row and padding follow-up:
1,920 core coordinates plus 229 ordered, wrapper, boundary, EOF,
source-event and bounded scale controls. Exact authored source bytes are
retained, with identical sources sharing native invocations (2,111 sources
for 2,149 coordinates). The native tree binds old text and executed
requests to their original owner. Reader rows, old interior/tail rows,
scalar padding witnesses, font/link owners, following section identity,
and JSON/ANSI transport are separate assertions. Each retained text block
must keep its original source, even when another block already matches.
Only source-proven motion forms can add one space-only reading row, and the
precise old literal bytes are required before `allowed-reading-space-row` can
pass.
Unknown extraction remains Review, with the native and product observations
retained; the original `--section-tail` and `--pending-rows` lanes are
unchanged.

`compare` also reports Markdown obligations by source hash, route, stage,
and axis. Both first readbacks are checked against the original transported
IR, even outside the strict native prose lane. The second readback has both
a source check and a stability check. The bounded projection distinguishes
scalars, author blanks, hard rows and tails, positive gaps, semantic
owners, and target/label ownership. Renderer framing and verified
standalone navigation markers have their own rules. Richer block shapes
without a selected projection remain Review. These statuses do not change
the direct native prose verdict or make a structured layout exact.

`navigation_evidence.py` proves generated destinations from raw Markdown and
source spans. `content_signature.py` projects independent owner and layout axes,
and `fidelity.py` compares those observations across readback stages.

`owner` still reports normalized path/kind topology; `glyph` still reports
the global scalar sequence. The separate `owner-content` axis compares each
mapped owner's non-layout scalars without Unicode folding. It is Review when
Markdown topology has no proven correspondence, even if the global sequence
is unchanged. Do not interpret historical `owner: exact` counts as proof of
text ownership. The local gate exercises 24 validated IR mutations through
direct comparison and both Markdown exporters (72 first observations). Both
second exports are checked against the source and their first readbacks:

```sh
cargo build --locked -p mant-engine --example markdown_owner_profile
python -m scripts.roff.audit.word_row_matrix.owner_mutations \
  target/debug/examples/markdown_owner_profile
```

The follow-up summary also records `nativeWord`: exact W-core rows,
unsupported literal-origin effects, protected ordinary-cell rewinds,
extraction limits, and actual differences. The two named Review states
retain the differing rows; they are never promoted to native passes.

Use `freeze --sections` for the section-entry and row-owner follow-up.
The manifest keeps 1,344 entry and 384 owner coordinates plus the fixed
chain, reset, pending-word and display controls. `nativeSection` records
the normalized predecessor, raw and filtered BODY children, admitted
spacing requests and actual native action for each source. The compare
summary reports resolved IR gap, heading and body row/owner axes separately.
Unmapped device rows and marker intervals remain Review. After product
collection, `section-ledger` writes the source receipt for each logical member
and separates templates confirmed by the actual tree from templates
normalized into a different branch. Source hashes join these rows to raw
native streams and the five product observations.
