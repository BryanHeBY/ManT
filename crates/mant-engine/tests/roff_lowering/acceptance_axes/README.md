# Roff acceptance consumers

The eleven complete sources in `cases/` have independent pristine CVS
`.expected` windows. The windows include the next section's spacing; the
axis cards state which rows, word boundaries, identities and styles are
applicable. `main_examples` executes the six main cards; `consumer_projections`
also exercises all eleven through real queries and Markdown boundary policies.
`consumer_ownership` checks their source owners and pending scalar boundaries.
Comparator mutations remain separate from product assertions.

`consumer_cases.json` is a second, source-bound cohort: seven mechanisms with
positive controls, the original empty-HEAD EOF input and its section variant,
Unicode cell ownership, and eighteen accepted-invisible-prefix variants.
It contains complete immutable sources, their SHA-256, native rows, admission
status, and all five pristine profile hashes. Its rows exclude the structurally
witnessed next-heading spacing or exact metadata footer. Interior, leading and
trailing completed blank rows are retained. Expected output never comes from
ManT. Regenerate or verify with:

```sh
python3 -m scripts.roff.fixtures.record_consumer_projections --check
```

The recorder verifies the registered pristine binary, runs every exact input,
and stores complete raw evidence under `target`. Removing `--check` updates only
native expectations; a changed source hash is rejected. Recovery diagnostics
are independent of the content assertions.

The consumer tests cover actual QueryBundle/outline/excerpt/explanation/search
JSON round trips, search UTF-8 byte coordinates, pending-cell scalar ownership,
ordered reference occurrences, source-owner AST checks, and ANSI layout parity.
The UI companion `mant-ui/tests/consumer_projection_boundaries.rs` reads this
same cohort and checks real buffers at effective content widths 20/40/78/120,
resize, search, styles, activation and exact visual copy. Unicode scalars,
UTF-8 byte offsets and terminal-cell columns are deliberately separate.

Every consumer retains the same accepted inline body, including each accepted
Lk colon and URI. Portable
CommonMark is not an IR serialization: native vertical-space blocks, responsive
field padding, and rich identities inside code fences are not reconstructed by
that exporter. Markdown assertions therefore test accepted content, rejected
suffixes and the explicitly applicable hard boundaries; JSON/native text/TUI
assert physical rows independently. The exact `column_tail_hard_row` source
exports its accepted cells as the logical fallback row `D | RightWord`; its
native physical cell boundary is checked independently. Other applicable
Markdown hard-row cards retain their strict row assertions. The one reviewed
final HANG seam can gain
an ordinary word separator or wrap responsively; earlier authored boundaries
are still exact.

The real Markdown reader also consumes the exporter's standard inline `br`
spelling for completed blank rows. A completed definition label uses a hard
break before prose, so reading that export cannot merge its first body row
back into the HEAD. The tag spacing case independently checks both blank rows
in both text projections; unrelated HTML remains preserved source.

These selected tests do not claim that all 8,788 acceptance identities have
exact source/style/scalar/query/UI coverage. Unit-level source spans absent from
the public IR remain unasserted; block/owner source facts and final-IR reference
origins are observed without inventing per-scalar provenance.
