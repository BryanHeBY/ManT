# Fo/Fa source adjacency contract

`cases.json` contains 129 complete, independently reference-derived roff inputs.
Every input ran the pristine pinned CVS mandoc binary with ASCII, UTF-8 at width
78, HTML, tree, and lint before its assertions were written: 645 profile runs.
The reference SHA-256 stored in the fixture is
`9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e`;
source and profile-output hashes accompany each case. The preserved baseline
CLI used to reproduce the original failures has SHA-256
`8f3d49ba0b7dcc0870458cb273b7526619db5200280dfc5e7ef20a1a75425f37`.

## Enumerated coverage

| Cohort | Count | Coverage |
| --- | ---: | --- |
| Core | 80 | Fa → Vt/Ft/Fd/Cd/In/Fn/Fo/Nm × ordinary/empty/bare `\z`/`\zX`/`\c` × SYNOPSIS/DESCRIPTION |
| Forward | 24 | Pf/bare Fl/explicit-empty Fl × same/separate input line × with/without Sm × both sections |
| Pairwise | 22 | Direct, Xo, definition HEAD, tag BODY, literal; Sm, ft, Tg, comment, invisible An; repeated/multiple Fa, nested Fo, external/manual links, targets, scope end, blank BODY line, truly empty BODY |
| Readable minima | 3 | Fa `arg\z` → Ft, Pf → same-line Fa, bare Fl → same-line Fa |

Sm is not callable in the pinned macro table. A same-line forward case executes
Sm before the prefix; a separate-line case places Sm between the prefix and Fa.
These cases do not claim an impossible same-line intervening Sm sibling.

Five lint runs exit 2 with recorded warnings: the four separate-line Pf controls
warn that nothing follows the prefix; `pair-blank-body-line` warns about a blank
line in fill mode. Other profiles exit 0. `pair-empty-body` is the distinct exact
`.Fo call` / `.Fc` input with no intervening physical line, and has clean lint.
Warnings are preserved rather than counted as clean lint passes.

## Execution responsibility

The corresponding pinned rules are `mdoc_term.c::print_mdoc_nodelist`,
`print_mdoc_node`, `synopsis_pre`, `termp_fa_pre`, `termp_fl_pre`,
`termp_pf_post`, and `roff.c::roff_node_prev/next/transparent`.

The complete original Fo BODY sequence determines adjacency. Fa's specialized
output route does not partition that sequence. Pf reads raw `n->next`; Fl and
synopsis pre read logical siblings, skipping only the native transparent set.
A visible scope or an invisible executable An remains a logical sibling.
Recursive Fo bodies have separate parent sequences, so inner arguments cannot
become an outer declaration's predecessor. Macro pre/post execute once before
publication of source-neutral content; no public IR execution fields are added.

The permanent engine tests compare native AST roles, coordinates, entry flags,
and parent owners; exact content and hard/empty rows after JSON; and actual
Markdown parser readback. Row comparison excludes device origins, terminal
padding and the outer section separator. It retains internal SP, TAB, NBSP,
scalars and empty rows. It does not use `split_whitespace()`.

## Explicit layout differences

Two bounded existing discrepancies remain outside this repair:
`pair-tagbody-ft-z` and `pair-target-ft` have native `LABEL` + three SP before
`call`, while the current reading projection has five SP. Native
`termp_it_pre` computes a `6n + 2 = 8` field, and LABEL occupies five cells.
Current IR retains an additional two-SP Fo prefix inside the BODY paragraph.
The tests verify the original LABEL owner, body indent 8, minimum gap 2, BODY
source line 10, that exact prefix, and all following text/hard rows. The fixture
records only that precise three-to-five-SP interval in
`known_layout_difference`. This is a known column discrepancy, not a claim of
native column parity or a generic whitespace normalization.

Two intentional Markdown layout projections are separate:
`pair-tagbody-pf` and `pair-linked-owner` serialize the native three-SP
HEAD/BODY interval as one SP. `definition_content` canonically joins shared
prose with one separator. `portable_definition_gap` identifies only that exact
interval; the original definition owner and every other scalar/space remain
checked.

## Final terminal consumers

The [UI tests](../../../../mant-ui/tests/source_adjacency.rs) have four exact
sources: the three readable minima and a short linked Fo body. They paint
complete Ratatui Buffers at 40 and 78 columns, check every body cell and font,
verify public search coordinates, and perform real reader copy and link clicks,
including both cells adjacent to the clickable label.

The native Fa/Ft minimum starts at device columns 5 and 9. The fixture stores
relative row indents `[0, 4]`; the UI uses its own three-column body margin.
JSON retains the native four-column second-row hint. Markdown keeps content and
hard rows while omitting this geometry, so its second row uses the ordinary UI
margin. Each supported body configuration, default and `preserve_semantics`,
is read by the actual parser twice and checked against the original expected
content. Neither configuration emits raw HTML anchors.

Addressable export has a separate contract: CommonMark receives HTML
destinations, while ManT intentionally retains unsupported HTML as literal
source. The codec finalization tests cover that distinction; this cohort does
not claim unchanged native Buffer rows after importing addressable HTML.

These bounded cases do not prove arbitrary roff behavior, full-page exact
columns, or every container/semantic combination.
