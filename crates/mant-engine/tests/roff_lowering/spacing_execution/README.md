# Spacing execution cases

The 191 exact sources and their expected rows were run through the pristine
pinned CVS binary before writing the assertions. `cases.json` retains the
reference identity and ASCII, UTF-8, HTML, tree and lint receipts per source.
Expected rows come from UTF-8 at width 78, with only the five-column device
origin measured by `ORIGIN` removed. Leading/interior SP, NBSP and empty rows
remain exact. The `ORIGIN` and `TAIL` rows bound the checked content.

The core is 72 filled and 72 no-fill cases: three final-on executions
(on→on, off→on, off→toggle), two preceding join states (`Ns`, `\c`), four
buffer states (visible, empty word, NBRZW, flushed), and three successors
(TEXT, `No`, `Lk`). No-fill begins before `ORIGIN`, so the NBRZW row is an
interior row of a complete literal owner rather than the separate documented
initial code-row projection. Ten independent report/mode controls, 24 ordinary repeated-off controls, and twelve
bounded container/state pairs cover definition HEAD/BODY, hang/inset,
display return, literal display, font/code successors, `\z`, `\zX`, and
authored SP/NBSP.

The execution rule is `mdoc_term.c::termp_sm_pre`: when final NONOSPACE is off
and the input buffer cursor `p->col` is nonzero, clear NOSPACE even if the
mode value did not change. `term.c::term_word` (573–589, 662–667) keeps that
cursor and the separate NONEWLINE continuation flag distinct. Empty words
and the actual buffered NBRZW cell therefore have different effects.

All 764 rendering/tree profiles exited 0. Of 191 lint profiles, 185 exited 0;
six container supplements exited 2 because native validation discarded a
redundant `.br` before `Bl`/`Bd`. Their complete diagnostics remain in the
receipts. These warning cases are not clean lint results. One inset
supplement retains its native NBSP row plus an explicit existing NBSP→SP
reading projection; every core/control row uses its native result unchanged.

Engine tests verify all 191 producer rows after JSON transport. The 179 direct
core/control sources also undergo two actual Markdown readbacks and complete
Ratatui Buffer checks at widths 40/78 after JSON and each readback. Six
filled linked successors additionally verify actual reader label fonts,
activation of every label cell, inactive adjacent cells, and exact selection
copy. Supplemental Markdown list/display geometry is outside this matrix's
exact row claims.

The repeated-off controls omit both `Ns` and `\c`: ordinary visible, empty,
NBRZW, and pure-SP first words precede TEXT/`No`/`Lk`, in filled and no-fill
mode. `termp_sm_pre` does not change NOSPACE when final NONOSPACE remains on;
repeating `.Sm off` must preserve the state left by the prior `term_word`.
The original explicit-`Ns` repeated-off control remains independently tested.

An additional on→off→off ordinary-word control preserves the incoming SP
established by the first actual mode change. This protects the repeated-off
fix without dropping the required on→off incoming-boundary behavior.
