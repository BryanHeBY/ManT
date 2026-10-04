# Column execution matrix

`cases.json` records 23 distinct exact inputs and all 115 pristine CVS profile
receipts. The primary binary SHA and active registry are in the header. The
original 105 runs live under
`target/audits/layout-convergence-20261004/lc4-column-review/records.json`;
the two additional hard-row inputs have individual `*-record.json` receipts
there. Assertions were written after these exact runs.

Every profile retains its original stdout, stderr, hashes, arguments and exit
class. `native_rows` removes backspace overstrikes only. It retains all hard
rows, completed blank rows, spaces and columns. `ast_cells` independently
records each original It BODY and its TEXT source locations and flags.

The native execution rule is `mdoc_term.c::termp_it_pre/post`,
`term.c::term_flushln/fill/field/vspace` and
`term_ascii.c::ascii_advance/locale_advance`: declarations choose 4/3/1 gaps,
excess intermediate fields have capacity ten without a gap, and each actual
advance is capped at 256. The last field may use the terminal right margin.

`cli_rows`, `ui_rows_78` and `reading_policy` describe the separately selected
reading contract. Signed actual parent and child origins survive source-order
stacking; an open literal tail may be reused while completed blanks cannot.
The UI stacks cells when their positions do not fit its viewport. Portable
reading does not retain the native terminal's right-margin soft wrapping.
`.ti` numeric geometry remains retired: its hard-row sources retain frozen
native gold but the reading expectations preserve only their actual hard rows.
The explicit JSON row-hint regression is a pure IR variation and makes no claim
of native `.ti` equivalence.

The UI owns a byte-identical package-local copy. The Python consumer fixture
observer checks the mirror, each source/profile SHA, native rows and cell count.
Tests never derive native gold from ColumnPreferences or candidate output.

`cell_boundaries.json` adds seventeen exact adjacent inputs and all eighty-five pristine
profile receipts, mirrored in the UI package. Their runs are under
`lc4-column-review/cell-boundaries/records.json`. They distinguish `.sp 0`,
`.br`, a real graph close, subsequent accepted words, empty/zero-width words,
font/navigation-only changes, filled/no-fill/literal owners, empty source TEXT
and rejected authored marker tails. `zero-cells-records.json` additionally
distinguishes an accepted NBRZW pass that ended an empty data row from an open
zero-width cell. A cell's `breakAfter` closes accepted
graph without adding text or a completed blank row. It describes the latest
tail, so later accepted content can reopen it. The original two source BODY
owners, raw oracle rows and profile hashes remain independent evidence.

Three transparent-wrapper neighbors ran under
`lc4-column-review/empty-container-tail-readonly/records.json`: an empty list
retains the previous executed close; a later accepted word reopens it; a
list containing an empty item retains its separately completed blank row.
`termp_bl_pre/post` only execute `term_newln()`, so zero-output topology
cannot revoke a physical endline. Empty/zero-width No after `sp 1` likewise
writes no accepted new graph; those two fixture `break_after` metadata facts
are true, with all original source and native profile bytes unchanged.

The four raw-blank literal sources retain the old composed hard-row
`payloads` view, while `inline_payloads` separately freezes accepted owner
scalars and `payload_close_endings` identifies its single generated close.
The helper composes that registered boundary only after checking the actual
`breakAfter`; this view is not author text or scalar coordinates. The second
literal blank's real `LineBreak` and empty TEXT witness remain in their owner.
