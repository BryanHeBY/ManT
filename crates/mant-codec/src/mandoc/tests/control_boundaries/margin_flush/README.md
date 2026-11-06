# Native margin-flush word boundaries

This finite matrix has 19 exact sources and 95 pristine reference profiles.
The original nine AR01 sources are retained byte-for-byte, with their old IDs
in `historical_id`. Ten opposite controls distinguish a real native buffer
from a word event and a pending formatter flag. Each source ran ASCII, UTF8,
HTML, tree and lint before its assertion was written; all 95 runs exit 0.

The primary rules are pinned `roff_term.c::roff_term_pre_mc`,
`term.c::term_word/term_fill/term_flushln/term_field`, and
`mdoc_term.c::termp_ns_pre`. MC tests native `p->col`: an empty or font-only
word under NOSPACE writes no cell; an automatic space, NBRZW or cached glyph
does. Bare BACKAFTER differs from a buffered zero-advance glyph. An actual
flush consumes its accepted/rejected input and sets minbl from trailspace.
It does not create an unconditional plain-text separator.

Coverage includes ordinary empty and font-only words, repeated MC, Ns/Sm
transitions, NBRZW, ordinary and fixed blanks, bare/cached zero-advance,
explicit hard lines, and Strong/Emphasis owners. No-MC and one-MC controls
really retain two automatic spaces; fixing repeated MC must not trim them.
The fixed blank remains U+00A0. Joined Ns controls retain FIRSTSECOND, rather
than accepting FIRST SECOND after whitespace folding.

Tests compare exact IR rows, real JSON decoding and two actual Markdown
reads. Existing field-retirement/spacing matrices separately protect the
declared TAG/HANG device geometry; this plain-field repair does not replace
those contracts or remove their preserved gaps. Engine and UI consumers
have independent representative query/copy/cell tests.

Re-record or verify with:

```sh
python3 -m scripts.roff.fixtures.record_margin_flush_boundaries \
  --evidence target/audits/margin-flush --check
```

The five-column common native section origin is removed once, by its fixed
device rule. Internal SP/NBSP and hard/empty rows are never folded. Fixtures
come from reference output, not the product being tested.
