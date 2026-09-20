# Native execution fixtures

These fixtures exercise the native-execution projection used by the production
roff lowering path.  The fixed CVS reference commands below remain independent
oracles rather than an alternate product renderer.

Before the K05 assertions were written, both table inputs were checked with
the pinned CVS binary:

```text
target/mandoc-migration/reference/mandoc -Tlint FIXTURE
target/mandoc-migration/reference/mandoc -Tutf8 -O width=32 FIXTURE
target/mandoc-migration/reference/mandoc -Tutf8 -O width=78 FIXTURE
target/mandoc-migration/reference/mandoc -Tutf8 -O width=120 FIXTURE
```

On `native-execution-table-interleaving.1`, all three widths render the first
row in alternating column slices: `left alpha` with `right one`, followed by
`left beta` with `right two`. The following row is `left tail` with
`right tail`. The `\p` controls are hard word-end breaks; the device rows used
to interleave the cells are not, by themselves, IR line breaks.

`native-execution-table-matrix.1` passes `-Tlint`. At width 78, the pinned
renderer shows three data columns, decimal-aligns `12.34`, keeps `span left`
across the first two logical columns, retains the empty middle cell in the
next row, draws the authored whole-row rule, and renders the final `T{` cell
with one authored break between `block one` and `block two`; the longer second
logical line then soft-wraps differently at all three widths. The first
`alpha` cell uses the layout's bold font. Numeric padding, box/rule glyphs,
and these device-only soft wraps are not semantic cell payload or IR breaks.

`native-execution-table-vertical-continuation.1` passes `-Tlint` and renders
one visible `first` row. The second data row uses tbl's explicit `\^`
spelling; fixed CVS `tbl_data()` treats it as the same vertical-continuation
fact as a layout `^` cell, so it owns no independent visible IR paragraph.
