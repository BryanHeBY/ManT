# Complete local name-field sessions

These repository-authored inputs protect the bounded tbl enhancement's entire
native source session. They supplement `source_sequences`, `source_adjacency`
and the historical synopsis observations; they do not replace those matrices.

| Family | Configurations | Independent evidence |
| --- | ---: | --- |
| Core | 25 | Five HEAD shapes × TEXT/Ft/Ar/Fl/No on a genuine following BODY line |
| Risks | 20 | Empty BODY vs same-line multiword HEAD, invisible/empty words, prior Fa/No/Fn, zero-advance/continuation, fonts, links, a new Nm, spacing and an enclosure |
| Original nested controls | 8 | Four complete fragments in an It HEAD and a Fo field |
| Additional ordinary observations | 3 | Nm in tag/hang/inset It BODY; retained as Review, not a passing field-layout claim |
| Cell controls | 4 | Adjacent columns, following row, pending glyph reset and distinct link owners |

The fixture records 113 complete source invocations, 109 distinct sources and
565 ASCII/UTF-8/HTML/tree/lint receipts. Each source and each raw stream is
SHA-256 bound. Canonical source construction is in
`scripts/roff/fixtures/name_field_session_cases.py`; the typed test helper in
`tests/support/name_field_sessions.rs` is shared by engine and UI consumers.

## Reference and classifications

The locked pristine reference is mandoc CVS 20261013T025419Z. Every exact source
ran before its assertion. The default recipe uses width 78 for ASCII/UTF-8,
`C.UTF-8`, UTC and exact UTF-8 stdin; it does not run ManT. Reproduce all raw
profiles without accepting a candidate:

```sh
python3 -m scripts.roff.fixtures.record_name_field_sessions \
  --check --evidence target/audits/name-field-sessions-oracle
```

The whole tbl form has lint 4, because native tbl does not execute these
high-level macros. Its unsupported operand projection is retained separately
from the selected bounded enhancement. The clean independent fragment has
lint 0 and a real Nm BLOCK/HEAD/BODY. Both sets of receipts remain available;
the whole-table text is never used as evidence of semantic macro support.

One exact independent source, `risk-prefix-Fn`, aborts the pristine HTML backend
at `html.c:650` with status -6. Its failure streams are recorded explicitly.
Its ASCII, UTF-8, tree and lint profiles succeed; no HTML behavior is inferred
from terminal output. Every other non-lint profile succeeds.

`term_ascii.c` chooses a five-column `defindent`; `termp_sh_pre(BODY)` selects
that section origin. Extraction removes exactly those five columns, the
device's trailing padding and NEXT's known section separator. It never derives
the origin from the minimum content indent or folds internal SP, NBSP or empty
rows. In particular unsupported implicit Nm retains its extra separator.

## Execution and consumer contracts

Pinned `mdoc_term.c::termp_nm_pre/post` installs the HEAD field, flushes it,
clears only NOBREAK/BRIND/HANG and trailspace, enters BODY at the name-derived
origin and performs its direct flush. This is one tcol/input/word-anchor
lifecycle. `termp_sh_post(BODY)` then runs the real final `term_newln`; an IR
owner drain does not substitute for that native boundary.

A recovered cell establishes ordinary Line execution and its actual dialect
before its first word. It does not borrow the enclosing It/Fo field geometry,
expand the closed language or alter the source/session budget. A declined
candidate retains its whole native cell; the companion codec test exercises
attempt/input/output-node/output-byte refusals and transaction rollback with
the exact `core-plain-Ft` source.

Engine tests assert actual parsed HEAD/BODY ownership, exact cell scalars and
CLI hard rows after JSON text roundtrip, two real Markdown readbacks and the
original eight ordinary nested controls. UI tests inspect real painted cells
at widths 40 and 78, native core B/I font bits, word search and the label's
copy/click range. Portable Markdown tables use a code-fence projection: native
inline font styling inside that fence is an existing explained difference,
while spelling, hard rows and stored navigation targets remain protected.

`risk-enclosure` has one additional **GeometryExplainedDifference**. Pinned Nm
BODY selects NOSPACE and advances its source offset by six columns; the word
is `int`, and the accepted IR carries row 1's `indentColumns: 6`. The engine
checks this exact hint separately from the scalar stream, and checks six
display columns in CLI and the Markdown fence. The unframed TUI cell has a
natural width of nine columns (six plus three). Its existing documented
`readable_origins` policy reserves four content columns inside that narrow
cell, reducing the presented origin to five at both tested viewports. After
fence readback the six SP are code content and remain six. Native gold is
unchanged; this presentation policy does not remove a word separator or
change any formatter register.

The three extra It BODY observations expose an existing structural synopsis
handoff difference, beyond the local recovered-cell entry. They are not
silently normalized into passing expected values: tag/hang break probe/int
where native shares a field; inset also loses the generated no-break space
between key and probe. Their exact native evidence is kept in this fixture
and the round's audit as Review. The unchanged eight It HEAD/Fo controls are
strict passing native comparisons.
