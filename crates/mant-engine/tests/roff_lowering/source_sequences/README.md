# Complete source-sequence contracts

These repository-authored sources exercise complete native parent sequences at
recovered tbl cells and bibliography fields. They supplement `source_adjacency`'s
Fo/argument-routing matrix, without replacing it or copying external manuals.

`cases.json` records 96 configurations:

| Family | Count | Selection |
| --- | ---: | --- |
| Recovered-cell core | 60 | SYNOPSIS/DESCRIPTION × Fa/No × ordinary/bare `\z`/`\c` × Ft/Cd/In/Fn/Nm |
| Cell risks | 16 | Empty/font/cached/invisible/blank words, raw/logical neighbors, controls, enclosure, links and multiword Nm |
| Bibliography HEAD | 6 | Sm on/off × one/two/three authors, title and URL |
| Bibliography root | 6 | Same author/spacing axes, followed by Sm on and a sentence-word control |
| Bibliography warning recovery | 8 | Both sections × ordinary/bare `\z`/`\c`/cached glyph |

Each original source and independent fragment ran ASCII, UTF-8, HTML, tree and
lint with the locked pristine CVS oracle: 192 sources/profile groups,
960 profile receipts. Eight additional actual-source controls exercise zero-cell
empty/font words and the opposite `\\c` continuation gate in both dialects;
two table-return controls retain the post's Sm-mode and next-word state.
These have one source each and 40 further receipts. The fixture binds the
oracle, exact sources and both raw
streams by SHA-256. Reproduce raw evidence without accepting a product candidate:

```sh
python3 -m scripts.roff.fixtures.record_source_sequences \
  --check --evidence target/audits/source-sequences-oracle
```

The recorder verifies the active pristine identity first. It never executes
ManT. Canonical inputs live in `scripts/roff/fixtures/source_sequence_cases.py`;
Python tests check their exact source identities and finite axis inventory.

## Contracts and diagnostic classes

The 60 core whole tbl inputs have lint status 4 (unsupported high-level macros),
while their independent fragments have lint status 0. The selected bounded
enhancement follows the independent fragment, not unsupported table geometry.
Native ft requests deliberately prevent semantic replay; that risk case retains
its complete base tbl output under a separate `native-fallback` contract.

The standalone Pf prefix has fragment lint status 2. Its retained nodes still
execute through the bounded enhancement and match native recovery; it is never
called clean. Only wrapper-classified safety findings reject this synthetic
parse. Invalid Ft bibliography recovery is separately recorded with lint 2.
Normal bibliography HEAD/root cases have lint 0 and retain conjunctions,
commas, final period, SENTENCE and URL identity under both Sm modes.

`mdoc_term.c::print_mdoc_nodelist` walks the complete native sibling chain;
`synopsis_pre` uses its logical predecessor. `term_newln`/`term_flushln` consume
backspace state at real boundaries. `mdoc_validate.c::post_rs` retains invalid
bibliography fields after reporting them. `termp____post` writes punctuation
and arms SENTENCE; unlike Eo/En, it does not clear NOSPACE after the word.

## Assertion boundaries

Recorded rows preserve internal SP and empty rows. Extraction removes only the
common native device origin, trailing device padding and NEXT's known section
separator. Native overstrike font bits remain independent evidence.

In is an established semantic Code projection. Its accepted Code spelling is
reconstructed from the independent native row after the witnessed `arg`, not
from an unexecuted authored `<`: continuation and zero-advance can change that
prefix, including the incoming SP. Tests check this Code owner separately
from native include Bold/Underline. Other source font cells compare directly.
Portable Markdown table fences retain content/hard rows, not cell styles.

CLI source projections preserve native bibliography rows. Portable definition
export uses the published one-SP shared prose boundary; only the witnessed
HEAD/BODY suffix gap is reduced. Internal HEAD spaces and independent hard rows
remain exact. The original-source TUI projection checks the recorded HEAD and
BODY origins, including native tag padding. After portable Markdown readback,
the unwrapped item uses the consumer's semantic BODY origin instead of that
native device offset; only this layout difference is excluded, not font/word or
hard-row facts. Readback peels only the documented outer bullet representation
of one native definition before checking item contents.

Engine tests verify JSON strings, owner structure, native text, actual two-round
Markdown parsing and typed references. UI tests paint real 40/78-column buffers,
check font cells, and exercise search, every external-label click cell, inert
edges and pointer copy. Representative bibliography HEAD/root controls also
reach actual buffers; longer author rows remain engine/native tests because
viewport soft wrapping is a separate consumer policy.

The actual-source controls keep every leading cell: `NODE_LINE` selects
NOSPACE even after a graphless word and an inactive literal owner. The same
gate must remain suppressed by NONEWLINE from `\\c`. No ordinary IR drain
performs either native transition. Table return retains NOSPACE and clears
NONOSPACE, even when the outer source previously selected Sm off. All eight
sources are clean in all five
pristine profiles and reach JSON, exact CLI rows, two Markdown reads, and
40/78-column buffers.
