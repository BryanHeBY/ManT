# Incoming word spacing

This finite matrix keeps three independent native facts: the current `Sm`
mode (`NONOSPACE`), the separator left by executed words (`NOSPACE`), and
the live input cells. Expectations come from the registered pristine
`cvs-20261013T025419Z` reference, never a ManT candidate.

The 110 configurations (109 distinct exact sources) were run in ASCII,
UTF-8, HTML, tree and lint before
the Rust assertions were written. Every profile exits successfully. The
fixture stores the exact sources, their SHA-256, both stream hashes,
diagnostics, and UTF-8 physical rows. The recorder removes only the measured
common ORIGIN prefix; it retains leading/interior/trailing SP, NBSP and
empty physical rows. Backspace-overwritten glyphs use the resulting visible
native cells.

## Coverage

- 30 core cases: empty, bold-only, previous-font-only, bare `\z`, `\z\c`
  operands under off→on / off→toggle-on, followed by TEXT / No / Link.
- 18 controls: on→on, on→off / toggle-off, repeated off, Ns, continuation,
  NBRZW, visible and consumed glyphs, author space, size/color escapes, real
  flush and no-fill. Bare `\z` consuming the next S is the pinned behavior;
  it is distinct from `\z\c`.
- 44 selected context cases: every core operand and transition in no-fill,
  filled display and KEEP; selected tag/hang/inset HEAD and BODY and diag
  BODY. HEADs use real Xo nodes and explicit vertical requests around the
  measured region. `-diag .It Xo` parses Xo as ordinary label text and is
  deliberately not claimed as extended-HEAD coverage.
- 18 direct-flush controls retain the incoming word register after a pretty
  Fn/Fo and distinguish an actual following `.br`. Empty/font-only/zero-width/
  visible operands, both mode directions and a linked successor are included;
  the exact two leading automatic blanks cannot be reduced to one as layout.

The engine compares exact rows after an actual JSON string round trip;
the direct core/control cases also pass two actual Markdown readbacks,
each compared to the original oracle rows. The flush cases preserve the
existing Markdown paragraph boundary between the pretty declaration owner
and the following prose owner: exactly one empty row after `call();` in both
readbacks. That boundary was independently recorded from the frozen baseline
binary before changing the assertion; it is not a claim of native row identity.
Every incoming word cell (including two automatic leading spaces), the
bare-zero-advance loss of S and the successor's link identity remain exact.
The UI paints Ratatui cells at
40 and 78 columns, and checks label style, copy and click ranges for the
empty, font-only, bare-zero-advance and direct-flush Link successors. Container Markdown
geometry is outside this fixture's claim; the native producer rows are
checked independently of Markdown's existing list/display policy.

The existing 191-case `spacing_execution` matrix remains unchanged and
protects nonempty Sm side effects, explicit joins, sentence/KEEP behavior
and established real-flush controls. These finite matrices do not claim
exhaustive combinations of arbitrary roff programs.

## Reproduction

`marker-blanks.json.gz` retains 256 exact sources from the finite word
matrix, with their source IDs and five pristine profile receipts. The
16 single-operand seam cases cover an earlier authored SP whose later
marker-adjacent SP is retracted by `encode1()`. The 240 ordered seam
cases cover a source SP surviving the restart after an ASCII_BREAK,
including `Bk`, `Sm off`, and `Ns`. Eight sources have a lint style
diagnostic and still produce normal native output. The test compares
untrimmed description rows after JSON and after two real readbacks in
each Markdown profile.

`borrowed-word-owners.json.gz` records 21 token-split cases, 18 adjacent
accepted leading-space cases and their controls, for 54 distinct sources
with five pristine profiles each. It covers a surviving automatic separator
after a projected zero-advance glyph, a deferred glyph plus held blanks from multiple `Ns`
operands, and the same pass boundary with an intervening `\:` or with a
plain glyph. It also covers separators that print in an intermediate pass
and an authored blank beginning in a later operand. Tight, ordinary and
coalesced operand controls check which spaces survive and where each
physical line ends.

Three native inputs have lint style diagnostics. Their terminal output
remains valid and supplies the expected physical rows.

The first 39 IDs are prefixes of their archived matrix source SHA-256.
Each case stores the full digest, exact input and both stream digests for
the five profiles. Replaying those inputs uses the `native-default`
recipe with terminal width 160. Remove only the known five-cell mdoc page
origin and the header/footer furniture from the UTF-8 description rows.
The Python fixture test independently checks the saved source identities.

```sh
python3 -m scripts.roff.fixtures.record_word_spacing
cargo test --locked -p mant-engine --test roff_lowering word_spacing
cargo test --locked -p mant-ui --test word_spacing
```

The recorder verifies all required oracle identities even when the binary
exists. `--write` is an explicit re-record operation; `--evidence DIR`
retains raw profiles beneath the requested task-owned audit directory.

Implementation references: pinned `term.c::term_word` (incoming separator
before mode update, including empty words), `term_newln` (sets NOSPACE),
`term_flushln` (retires cells without resetting the word-end register), and
`mdoc_term.c::termp_sm_pre` (only nonzero live cursor releases NOSPACE when
the result is on). Native enclosure post handlers may also release NOSPACE
without creating a word; output-owner retirement does not.
