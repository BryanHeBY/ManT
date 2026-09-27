# R05 annotated-reference checkpoint

Status: R05a design and evidence checkpoint, not a claim that compatible
references are implemented. Branch `dev`, clean baseline
`b926eeb2c65b6630524385547d94812d81e0b5a4` (2026-09-27). The default
roff entry still uses Flow; `--annotated-preview` selects Fixed. No version
increment or production-entry switch is part of R05.

## Reproducible identities and existing coverage

- Pinned CVS: `crates/libmandoc-rs/vendor/mandoc-cvs-20260920T122115Z`.
  `scripts/mandoc-oracle-preflight` passed with tracked attestation
  `crates/libmandoc-rs/upstream/oracle/attestations/cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1.json`,
  binary `target/mandoc-migration/reference/mandoc`, archive
  `target/mandoc-migration/freeze-cvs-20260920T122115Z/upstream-cvs-20260920T122115Z.tar.gz`,
  profile `utf8`. Binary SHA-256:
  `d7c58752e2586695d17f4a87cc5540b35ec65ebec51bbcb5de8c9e6786e7311d`.
- Frozen same-HEAD release CLI:
  `target/entry-r04-compare.NO3rLu/mant-b926eeb2`, SHA-256
  `c1eb2185cf767f2d52d7cb2e15c989a38bad6ac05de7b0fdf83df4df1ba7336e`.
  Built with `cargo build --locked --offline --release -p mant --features
  annotated-preview`. The 79 current Fixed query gold cases passed via
  `scripts/annotated_fixed_query_gold.py` against that binary. This is an
  entry baseline, not an R05 link pass.
- Same-HEAD prior entry/performance evidence is preserved under
  `target/entry-r04-compare.NO3rLu/`; it is not a new R05 measurement. R05d
  must capture paired candidate measurements with identical inputs and mode.

The prior same-HEAD direct-text medians below are starting context only, not
R05 candidate results. Times are milliseconds, peak RSS is KiB, and each
Flow/Fixed pair used the same fixture hash and release binary:

| Page | Input SHA-256 | Flow / Fixed wall | Flow / Fixed RSS |
| --- | --- | ---: | ---: |
| GCC | `a86bd1d671aa2afff558eb3b7c8b6e2a8bbaf55cb9fb2a6549b7538dd56e0f0f` | 308.431 / 406.606 | 90208 / 58374 |
| Git | `2736b9d20cd9a36c7971ccdbd7ba5e71cea543960340bb9a27984369df559862` | 12.685 / 18.601 | 14894 / 11838 |
| Clang | `ff10a1611fc293291388feabf2d988c2c8950c0e784a34d4fcea5b4eb966fce0` | 9.015 / 10.347 | 13150 / 10650 |
| rclone | `f35de3b3008f684a7db141a7626db68b08c46e5b3d45c97726bc6d97eebade4e` | 544.492 / 747.163 | 244180 / 144316 |

The Flow/Fixed entry counts were respectively GCC 4916/3674, Git 167/167,
Clang 89/89 and rclone 0/0. R05 compares Fixed before/after for output and
entry invariance; a Flow/Fixed comparison alone cannot establish that adding
compatible links changed no native display byte.

Current explicit-link chain: `mant_mandoc_annotated_collector_marks.c` owns
active marks and phrase lifetime; `mant_mandoc_annotated_collector_buffer.c`
ties labels to live slots; `mant_mandoc_annotated_display.c` seals surviving
output; `mant_mandoc_structured_link.c` decodes explicit targets. The
`libmandoc-rs` annotated FFI transfers checked views, codec
`annotated_fixed/marks.rs` projects them to Fixed IR, and existing
query/protocol/CLI/TUI consumers use the typed occurrences. R05 adds a
compatible-candidate producer to this chain, not a second link store.

The last sentence describes the typed Fixed IR and direct TUI document path,
**not** completed Fixed reference-inventory projection. An independent
same-HEAD check found an existing R05d prerequisite: with the exact input
`.TH T 1` / `.SH SEE ALSO` / `.MR printf 3`, pinned CVS displays
`printf(3)`. The frozen CLI's Flow `--outline-references all` reports one
exact Manual occurrence, but `--annotated-preview` reports
`occurrences=unknown/not-scanned` and no records. The reason is explicit in
`mant-query/src/projection/outline.rs::reference_inventory` and
`projection/references.rs::project_references_with_limits`: both return
unscanned for `document.flow().is_none()`. Existing `ReferenceRecord.origin`
and `ContentProjection` are Flow coordinates/stores, so enabling this for
Fixed requires a checked Fixed occurrence coordinate and bounded response
projection, plus consumer updates. It must not pretend a Fixed occurrence is
an `Inline::Link` or fabricate a Flow body. R05d cannot claim end-to-end
outline/reference coverage until this is implemented and tested.

As the first R05d bridge, each Fixed `LinkMark` now retains the native
section/owner context at the occurrence's start. This is checked against the
typed section and owner tables on IR validation and survives JSON roundtrip.
It is not a Flow location, a copied body, or by itself a completed reference
inventory; later response projection can use it to select the correct read
scope without guessing from terminal columns.

The next R05d candidate projects checked Fixed link marks into a distinct
`fixedRecords` response array. Its native key, section/owner, first final-run
byte slice and source evidence remain explicitly Fixed coordinates; it does
not create a Flow content store. The bounded scan supports document and
selected section/owner scopes, typed target filters, occurrence paging and
source-read selectors. Text/JSON/MCP presentations and TUI reference rows
consume these records or the same validated Fixed surface. The TUI attaches
reveal anchors to the first surviving label slice and opens/copies the typed
target. The protocol snapshot, native-to-query and CLI process tests, TUI
activation/copy tests, strict targeted lint, and 79 Fixed entry gold cases
passed for this vertical bridge. It does not by itself close the R05 link
coverage, full resource or release gates.

Pre-change directed tests on the current source passed: native
`annotated_links.rs` 15/15, codec `annotated_fixed::tests` 169/169, and
`mant-ui` reference-filtered tests 33/33 (plus one real Git integration
test). These cover existing explicit links and consumers, not the missing
Fixed inventory or R05 compatible links.

## What the fixed CVS establishes

The fixed reference was run with these exact inputs, once with `-Ttree` and
once with `-Tutf8 -O width=78`:

```roff
.TH LINKS 1
.SH DESCRIPTION
a(1) \%<> and b(2) \%<>
```

The third line is one text node, yet displays `a(1) <> and b(2) <>`. Node
identity alone cannot identify two target/label ranges. The second probe:

```roff
.TH LINKS 1
.SH DESCRIPTION
.BR a (1)
\%<>
```

has a `.BR` element with operands `a` and `(1)` and a separate text node for
the marker; the display is `a(1) <>`. `man_term.c::pre_alternate` emits the
operands without an inserted intra-macro space. `term.c::term_word_node`
sets the current node for one `term_word()` execution, while
`term.c::term_word`, `encode`, `encode1` and `bufferc` execute escapes and
emit buffer/glyph events. `roff_escape.c` classifies `\%` as ignored; the
terminal path produces its nonbreaking zero-width control and leaves `<` and
`>` visible. These paths prove display and execution facts, not that CVS
terminal emits a typed implicit link. Promotion is ManT's conservative
annotation policy.

Further exact-input `-Ttree` and `-Tutf8 -O width=78` probes were run before
writing any R05 behavior assertion. The following strings are normalized
visible body text; styled UTF-8 output also contains native backspaces:

| Input body after `.SH DESCRIPTION` | CVS tree/execution fact | Visible body |
| --- | --- | --- |
| `a(1) \%<> and a(1) \%<>` | one text node, two equal spellings | `a(1) <> and a(1) <>` |
| `.BR printf (3)` | one BR node, two text operands | `printf(3)` |
| `.IR printf (3)` | one IR node, two text operands | `printf(3)` |
| `.BR a (1)` then `.br` then `\%<>` | BR, hard-break macro, separate marker node | `a(1)` and `<>` on separate rows |
| `.nf` then `a(1) \%<>` then `.fi` | text node carries `NOFILL` | `a(1) <>` |
| `a\-b(1) \%<>` | escape retained in one text node | `a-b(1) <>` |
| `a\fBb\fP(1) \%<>` | font escapes retained in one text node | `ab(1) <>` |
| `.de REF` / `a(1) \%<>` / `..` / `.REF` | expanded text node is at invocation line | `a(1) <>` |
| `a\zX(1) \%<>` | overprint escape retained in one text node | native stream contains backspace |

These probes do not assert that the current Fixed path recognizes any of the
compatible references. The `NOFILL` and hard-break cases are rejection
policies; the tree and stream only establish that the text still displays.

## Private candidate mapping frozen for R05b

An input segment is `(term_word invocation, node, [start_byte,end_byte))` in
the actual UTF-8 word input. It is not a file offset, terminal buffer slot,
display column, or `term_collector_event.pos/end`; those event fields refer
to terminal state. One ordered candidate may contain several segments, with
separate label and Sphinx-marker segments. Distinct candidate identities
remain distinct even in one node or on repeated execution of that node.
An authored `SourceSpan` is attached only when proved by source accounting;
macro-expanded text cannot borrow fabricated authored byte coordinates.

At the common `term_word`/`encode`/`encode1` consumption boundary, an
ephemeral tag associates consumed input subranges with emitted glyph events.
It must advance with actual escape execution rather than decode the source a
second time. A source escape can produce zero, one or several glyphs. The tag
travels with the existing slot through move, partial consumption, overwrite,
and release. No stable ABI token or second writer is exported. On final
device output, only surviving tagged glyphs map to checked UTF-8 `OutputSlice`
fragments after font folding and overprint. The final result retains the
occurrence, target, source evidence and useful fragments, not a page-long
execution trace.

Read-only code audit of that hook: `term_collector_event.pos/end` currently
name terminal-buffer positions, not word bytes. A minimal private upstream
hook must report a synchronous word-enter/leave identity and the consumed
half-open input-byte interval on each logical glyph event. Plain `encode()`
emits per input byte; an escape's complete consumed sequence labels all of
its emitted glyphs, including font overstrike strokes, while an escape with
no glyph yields no label. Leading `AUTO_SPACE`, formatter-generated words and
layout writes have no candidate input interval. The hook's word pointer is
valid only during the synchronous callback; the collector retains keys and
bounded offsets, never that pointer. This requires a replayable local vendor
patch, not an FFI-visible pointer or a reinterpretation of existing fields.

`mant_mandoc_annotated_collector_buffer.c` currently chooses `pending_link`
at `TERM_COLLECT_LOGICAL`, copies it into the active slot on buffer write,
and transfers the surviving slot label at `FIELD_PLACE`/direct output. R05b
must choose the candidate's key at that logical boundary, leaving the slot
state machine as the only active writer. The existing selection builder scans
only final coalesced runs, so a label transition inside one text node produces
separate checked parts without retaining retired glyphs. Current FFI accepts
kind-3 marks with a typed target; codec `project_link()` converts those marks
to Fixed IR. The R05 compatible producer should feed that existing path,
with native source coordinates attached only when authored provenance is
proved. Existing coverage supports link `rejected`, `unverified` and
`ambiguous-survival` reasons; a single native rejected-link boolean alone is
insufficient for R05's per-reason failure matrix.

The first R05 infrastructure unit adds replayable patch 0043 to report the
synchronous word interval, and the collector checks plain `TEXT` byte/event
identity before any compatible candidate uses it. Inconsistent optional word
evidence revokes link evidence without rejecting the checked display. This
unit does **not** recognize Sphinx or styled references. Its candidate
measurement, binary hashes, alternating-run timing and exact text hashes are
retained in `target/r05-word-hook-measurements.md`; those target artifacts are
not packaged or treated as a completed R05d performance result.

The next scoped R05b unit admits a literal two-operand `.BR name (section)`
or `.IR name (section)` as a compatible Manual occurrence. The pinned
`man_term.c::pre_alternate()` operand identity and the final-surface label
selection are used; trailing punctuation is excluded. The source grammar is bounded
and deliberately conservative: escaped operands, multi-pair macros and
Sphinx markers still await the complete candidate-to-glyph mapping. This is
only a partial L02 result, not closure of L01–L10. Its exact four-page text
hashes match the frozen baseline; 79 Fixed entry gold queries pass. A small
same-input paired release probe showed no obvious time/RSS regression; raw
numbers are retained in `target/r05-styled-manual-measurements.md`.

The next R05b candidate uses the synchronous word-byte hook to admit
source-marked `name(section) \%<>` text references, including distinct markers
inside one text node. It checks escaped `.BR`/`.IR` names against surviving
glyphs and revokes a styled candidate whose name has no surviving style
evidence. Text-node candidates retain `SourceKey` without inventing a source
column after expansion. A revoked compatible mark does not consume a Fixed
typed link key, and the checked native body remains intact. These are scoped
native/FFI/Fixed IR capabilities, not yet Fixed reference inventory,
CLI/TUI end-to-end acceptance or closure of L01–L10.

For this worktree candidate, all `libmandoc-rs` and `mant-codec` all-feature
tests, strict two-package Clippy and 79 Fixed entry gold queries passed.
Direct-text SHA-256 matched the frozen release baseline on GCC, Git, Clang
and rclone. The release binary SHA-256 was
`c50f1ce1b772d6d1d095fec7493e8c53dfa4fda7dfdbfe9f3e6367a222e4549e`.
Three alternating same-input timing pairs (seconds / peak KiB) were GCC
baseline `.42/.40/.40`, candidate `.38/.38/.38`, and rclone baseline
`.74/.73/.74`, candidate `.70/.70/.70`; RSS stayed near 58 MiB for GCC and
144 MiB for rclone in both. This small probe is not the final R05 performance
gate or a claim of a statistically established speedup.

Mapping is monotone per executed input and bounded by actual input, active
candidates and emitted glyphs. UTF-8 boundaries, execution identity,
ordering and final selection containment are checked. A completely
understood native overwrite can legitimately remove a glyph. If a committed
candidate's necessary label cannot be mapped unambiguously, revoke that
candidate alone and record a link-dimension coverage reason and bounded
diagnostic; never recover by final-text search or widening to the whole
node. Weak candidates rejected before commitment are normal conservative
non-matches. Budget, memory, unsafe source or display faults remain hard
failures under the main contract.

Across nodes, source marker or macro-operand evidence and checked `TextJoin`
must both establish the full candidate. `.BR a (1)` plus separate `\%<>`
can yield one occurrence whose label is only `a(1)`; the marker and its
separator remain visible and non-clickable. A matching styled candidate is
merged with it, whereas two same-name occurrences at different positions
remain distinct. Hard/unknown joins, unrelated owner/cell boundaries,
no-fill/code and overlap with an explicit link conservatively reject the
compatible candidate without altering display.

The R05b proof panel must include: two different and two identical targets
within one text node; the cross-node `.BR` probe; soft wrap versus hard
boundary; escapes and font changes before and inside candidates; overprint;
macro expansion; and both benign weak rejection and committed mapping
failure. Every behavioral expectation must first be checked against the
exact input in the fixed CVS, then through native, FFI, IR, query, CLI and
TUI. L01–L10 in the guide remain open at this checkpoint.

The R05 link-coverage unit keeps one private failure flag on each affected
native link mark: rejected destination decoding and ambiguous committed
candidate survival remain distinct. The existing bounded coverage table
projects each failure to its actual region/owner/source when proven, with an
authored source position only when valid; after wholesale annotation stripping
it reports only a document-scope gap. C result checks verify reason, ordering
and scope against the failed mark. FFI and codec carry the issue into a
semantic-coverage diagnostic, while an ordinary weak candidate remains a
conservative non-match rather than an internal error. Synthetic post-render
fault tests verify that bad optional link facts remove clickability without
altering native body bytes or unrelated references; they do not claim a new
roff spelling naturally triggers a decoder fault.
