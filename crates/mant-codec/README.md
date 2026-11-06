# mant-codec

In-memory source decoding and portable Markdown encoding for `ManT`'s semantic IR. The default build supports Markdown, semantic annotations, and tldr without a native parser or C compiler; the explicit `roff` feature adds libmandoc-backed man/mdoc lowering.

```rust
let parsed = mant_codec::parse_markdown("## Example\n\nHello.\n", None)?;
assert_eq!(parsed.document.sections.len(), 1);
# Ok::<(), mant_codec::MarkdownParseError>(())
```

## Boundaries

Callers supply source bytes or text and optional source labels. Labels never grant filesystem access. The roff byte entry point uses plain input and disables implicit includes; loading, decompression, redirects, configuration, and catalog discovery belong to `mant-loader`. Native parse reports remain owned values; this crate exposes no raw FFI pointers. The source boundary does not promise that native internals make no operating-system calls: `libmandoc-rs` may use private diagnostic capture files, but cannot open a supplied source label or follow includes through this codec.

Semantic recognition, explicit name bindings, declaration groups, and identity allocation happen once during production. Queries consume the resulting facts rather than recognizing entries again. Markdown extensions add metadata without replacing the original visible content. Semantic Markdown export is a checked subset, not lossless IR serialization.

Document encoding borrows existing IR. Addressable artifacts keep canonical text and source-owner mappings together. Report fragments may decorate one inline root without altering canonical artifact coordinates. This crate does not query, render terminal themes, read configuration, update sources, or launch processes.

Markdown inline encoding separates bounded delimiter selection (`styles`),
final row framing and seam serialization (`serialization`), and an independent
accepted-child meaning check (`projection`). Validation sees the final bytes;
one bounded recovery may retain representable child styles without accepting
the candidate's own reinterpreted delimiters as its expected meaning.
Retained links carry the accepted label meaning and original destination/title
through bracket serialization and final framing. Link metadata has its own
escaping: literal entities and control whitespace never become visible rows.
Unrepresentable nested link ownership remains outside the portable subset.
Ordinary Text is always literal, including angle-bracket URI spellings. Only
an existing typed Link can emit an active wrapper, after checked serialization;
invalid external spaces are never repaired into `%20`. Policy-omitted and
invalid wrappers leave their children in the surrounding inline context.
Typed tldr `more_information` fields use explicit URI encoding independently
of literal descriptions and examples.

Markdown admission masks terminal-unsafe controls both in original source
bytes and in decoded Text, Code and link titles. Decoded findings retain the
original content-event range or effective reference-definition range, not a
fabricated decoded-character offset. TAB, LF and CR remain allowed content;
ordinary phrasing Text/label CR is entity-encoded on export so it cannot change
Markdown source lines. Metadata stays outside
visible-row framing. Destinations containing controls remain invalid in IR;
safe export preserves their labels without creating an activatable wrapper.
Raw-source masking precedes syntax parsing and is not a destination-identity
repair mechanism. Bounded changed ranges and the original parser's reference
bindings guard target admission; an unprovable identity retains its label with
a source diagnostic. Label/title display recovery does not by itself refuse an
unchanged destination. Only damaged inputs require an extra original-source
first pass; bounded local checks and callback copies share one explicit quota.

Encoding hand-built IR is a checked projection without mutating that IR:
unsafe content/title controls are masked, genuine code/fence CR and CRLF become LF,
and multi-row `Inline::Code` retains its existing LF hard rows. The separate
`commonmark_code_span` helper encodes one syntax span, whose CR/LF/CRLF become
spaces. These reductions do not make Markdown a lossless IR transport.

Portable tables project ordinary Text/label CR as U+FFFD and retain LF; code
and literal content keep their own CR/CRLF-to-LF rule. Fences only frame the
accepted reading payload. Addressable owner ranges are composed from those
same final bytes, including growing or shrinking transformations. Original IR
and JSON remain unchanged.

Routine tests include 13,729 delimiter-risk configurations and 2,720 Markdown
link-source configurations, each checked twice against original IR. The full
252,362-configuration corpus has an explicit release entry:

```sh
cargo test --locked -p mant-codec --release --all-features --lib \
  complete_phrasing_link_matrix_has_252362_original_ir_two_round_cases \
  -- --ignored --nocapture

cargo test --locked -p mant-codec --all-features --lib \
  link_metadata_and_bracket_context_matrix_use_original_ir_for_both_readbacks \
  -- --nocapture
```

Its style counters distinguish first-read loss, further second-read loss and
any loss relative to original IR. These are separate from scalar, code and
link-ownership failures; a permitted style reduction never permits marker text,
target/title changes or a shifted link range.

## Public entry points

| Need | API |
| --- | --- |
| Parse caller-owned Markdown and an embedded quick reference | `parse_markdown` |
| Parse an already-loaded quick reference or command template | `parse_tldr_page`, `parse_tldr_command` |
| Parse prepared, decompressed roff bytes | `parse_roff_bytes` with `roff` |
| Return IR plus the same owned native parse witness | `parse_roff_bytes_with_report` with `roff` |
| Lower an already-owned native report | `lower_mandoc_document` with `roff` |
| Recognize standalone alias syntax without resolving it | `redirect_target` with `roff` |
| Encode complete document content | `encode::render_markdown`, `encode::render_markdown_with_options` |
| Encode canonical bytes and source-bound coordinates | `encode::render_addressable_markdown` |

`TldrPageLocation` and roff `Path` arguments carry source identity, not loading
authority. `MarkdownParseError`, `TldrParseError`, and native
`libmandoc_rs::ParseError` describe decoding failures; file acquisition errors
remain with the loading caller. The native-witness API parses once; lowering an
existing report performs no second parse. `encode::MarkdownFragmentOptions`
deliberately excludes semantic declaration metadata because detached fragments
have not passed whole-document representability checks.

TLDR `More information:` metadata retains the destination of one explicit
`CommonMark` autolink or Markdown link, not its label or surrounding sentence
punctuation. Dots authored inside the destination remain part of the URL and
stay inside the autolink when re-encoded. Bare URL metadata is not heuristically
trimmed; ambiguous multi-link lines retain the display fallback. Host activation
allowlists still belong to the reader, not this codec.

## Native lowering ownership

Native lowering internals live under `src/mandoc/`, behind the optional `roff`
feature. Public entry points return the shared IR, not a second document model.
The following boundaries keep source interpretation shared across prose,
literal displays, lists and table recovery:

| Responsibility | Owner and lifetime |
| --- | --- |
| Stage composition | `mandoc/mod.rs` resolves the parsed document and runs lowering, navigation and validation. |
| Source lookup | `source_context.rs`, `ast.rs` and `equations.rs` provide source/AST services and bounded operation-local memoization; diagnostics use their own collector. They do not store a hidden formatter register. |
| Formatter state | `FormatterState` carries font, spacing, deferred vertical-space debt, and bare zero-advance execution explicitly between consumers. A normal font-scope exit restores current font but retains previous-font effects; native tables reset cell-local backtracking at their documented boundary. |
| Container routing | `containers.rs` streams borrowed children and scope boundaries; structural payloads remain tables/lists. Logical punctuation adjacency is a separate, non-executing classification in `adjacency.rs`. |
| Scope posts | `containers/posts.rs` retains the single shared BODY post, font-checkpoint and display-fill record across output destinations. |
| Inline and physical lines | `InlineBuilder` executes word/control events; `source_cursor.rs` places source-visible events on physical lines. No-fill changes layout, not macro interpretation. |
| Text execution | `inline/text_execution/` consumes decoded events in order using borrowed font and zero-advance state; native writes precede presentation. `roff_escape/` owns event types, decoding, pure width measurement and the shared zero-advance machine. |
| Plain input retirement | `inline/flow/plain_input/` consumes ordinary or authorless input at real native boundaries; presentation is separate from the consumption decision. |
| Output ownership | `inline/flow/output/` separates checkpoints, scope annotations, physical hard breaks and drains. Draining IR does not consume native input. |
| Scoped row origins | Content-owner coordinate bases are independent of the live native row. Scope checkpoints restore the base; new owners rebase accepted output only. Private carriers preserve signed origins through annotation and root materialization. |
| Nested list execution | `inline/scopes/nested.rs` executes nested list and column phases in the caller's existing builder and field state. |
| Link identities | `inline/links/identity.rs` applies HTML attribute glyph rules independently to each source operand; it never consumes the visible formatter's state. `Xr`/`MR` share this path with `UR`/`MT`/`Lk`/`Mt`/`%U`. |
| Link annotations | `inline/links/annotation.rs` places typed links around accepted output, keeping preceding glyphs and layout boundaries outside the label without executing another formatter boundary. |
| Deferred annotation members | `inline/ownership.rs` and `ownership/annotations/` retain born font and source membership independently of native glyph survival. Operation-local occurrence keys reference one immutable destination; accepted equivalent runs materialize once at each representable source identity. Real root closure ends new admission without changing older pending members. Private receipts never become public anchors or targets. |
| Layout measurement | `layout/measurement.rs` shares pure native-list unit/sample measurement; it does not execute sample escapes in the live formatter. |
| Structural layout | Block drivers own pending paragraphs and list state; section, synopsis, man no-fill and dialect-specific list consumers remain separate. Shared definition helpers do not own formatter state. |
| Speculative recovery | Table-cell candidates retain output, final formatter state and diagnostics until ownership acceptance. Rejection rolls back all three, unlike a normal font-scope exit. |

Definition HEAD lowering returns named output and execution outcomes rather
than positional boolean tuples. Native list measurement shares one pure
unit/sample helper; its reading-width approximation remains separate from the
native `term_strlen` contract. Display paths share explicit BODY font/fill
ledger stages, while their source-node entry, sink routing, post-once claim
and final geometry restoration remain ordered by their existing drivers.
No helper return or destructor implicitly executes a native scope post.
`blocks/lists/definition/` keeps the ordered item driver in `mod.rs`, with
HEAD execution, BODY handoff and resolved layout in sibling modules.

Entering a native field moves the existing plain input buffer and stable word
owners; it does not create an empty parallel buffer or settle pending glyphs.
The private `PlainInputUnit` groups those cells, owners and output cursor.
Its explicit operations distinguish detaching output coordinates, moving live
input into a field, clearing consumed input and retiring a physical row with
its tab origin. Authorless fields retain the ordinary output interval; these
operations do not reset font, continuation, keep or zero-advance registers.
`inline/synopsis.rs` applies source-sibling pre rules for prose, literal and
definition destinations. `Nm` retains its distinct nonempty HEAD entry, while
`Fn`/`Fo` share the real temporary name-field and argument-flush lifecycle.
Declaration paragraphs are released only after those execution boundaries;
their semantic grouping does not simulate another pre or post.

Special function-argument routing borrows the complete original parent sequence.
Its raw successor (for `Pf`) and logical neighbours (for `Fl` and synopsis
entry) remain distinct, including across argument boundaries and transparent
requests. Nested functions never replace their parent's sibling context.
Recovered table-cell bodies and bibliography bodies borrow that same context;
an isolated generated operand does not pretend to be a source sibling list.

Accepted device padding remains distinct from authored scalars until its
annotation boundary is known. Moving a leading gap outside a link also
remaps live output checkpoints using addresses recorded during projection,
not a text diff. A generated inset/diag BODY word observes its own native
cell interval; only actual field acceptance can establish its visible seam.
Projection-only owner drains cannot supply that evidence.

One-line display presentation chooses an IR destination independently of
source fill mode. Its real post closes the live row before geometry returns.
Following punctuation uses ordinary shared execution, never an independent
lowering pass that edits an earlier completed content owner. Filled source
line observation likewise cannot override the spacing state used by actual
macro and text words.

`Sm` executes a native request, not just a Boolean assignment: even an already
enabled `Sm on` releases NOSPACE when the live input buffer contains cells.
An empty word's boundary register is distinct from an actual zero-width native
cell. A retired buffer cannot borrow occupancy from already presented output;
continuation and KEEP retain their own lifetimes.
The incoming separator is consumed before the current spacing mode establishes
the next word's register. Even a zero-cell word executes that transition, so
enabling spacing on an empty buffer cannot invent a separator retrospectively.
NOSPACE belongs to the shared execution state, independently of plain/field
cell storage. A direct `term_flushln` consumes input without resetting this
register; a real `term_newln` selects NOSPACE even when no output row is active.
Decoded overstrike joins and field-pass row projection do not execute either
native flag transition.

The temporary no-break flush used by `.mc` tests live input cells, not the
presence of previously projected glyphs or an executed empty word. Ordinary
prose retains native trailing-space distance independently of the next word's
automatic spacing; a flush does not invent an irrevocable separator.

A native field scope can span multiple paragraph and literal content owners.
Its explicit source pre/post boundaries protect the live word, separator and
field registers while an IR drain releases only output indices. Resolved BODY
placement remains a layout fact; it does not establish an execution lifetime.
Ordinary `It` and `Nm` parts use that same session: `Nm` HEAD pre/post and
BODY pre/post run against the surrounding field, with normal block traversal
inside BODY. Changing an IR owner neither flushes that field nor reinstalls
the enclosing HEAD configuration. Source-line entry runs against the active
output before choosing paragraph or literal presentation. Native source
origins and relative IR owner origins stay distinct; accepted hard-row and
soft-wrap origins use the existing layout fields.
Declaration content keeps its paragraph identity when native execution enters
no-fill; explicit displays still choose literal owners. This private output
role selects a destination, not another interpreter or a saved fill mode.
Actual macro pre/post and backend line endings consume native input before
retiring its projection; a plain IR drain preserves unconsumed requests.

HEAD/BODY selectors borrow structural views, not an exhaustive content stream.
Macro drivers execute their direct siblings in order and apply fonts, field
flags and posts at the original parts. Definition prefixes stay with their
term owner; section display prefixes do not alter HEAD-derived navigation
identities. Native sentence flags are consumed by actual word writes, including
generated words, rather than inferred from visible punctuation or output drains.
Native posts update the live field configuration before an owner drains;
rebuilding an output destination cannot restore cleared HEAD flags. A source
line gate is consumed once for its actual AST node, while completed literal
rows retain their origin through output assembly.

Source geometry retains bounded basic-unit positions independently of the IR
parent and man macro base. Conversion produces signed relative offsets;
ownership normalization rebases only transferred roots. Text presentation
composes each parent once. It keeps resolved gap requests separate from literal
newlines through nested flow composition, sharing cell/marker/gap rules with
the UI through `mant-ir::geometry`. Zero block spacing is tight, not a
frontend default; each source request has one consumption point.

Manual link names and sections are separate identities. Each operand starts
with fresh skip state; an explicit empty section remains empty and invalid,
while an omitted section remains unspecified. This contract does not emulate
the bytes of arbitrary CVS HTML link templates: `print_href()` shares
`HTML_SKIPCHAR` across template expansion, so name `printf\z`, section `3`
and template `%N.%S` produce HTML href `printf3`. The typed target keeps name
`printf` and section `3`. `Sh`/`Ss` heading identities and `Sx` destinations
instead use their shared authored `deroff` phrase namespace. Visible labels
execute once in the surrounding formatter, independently of both namespaces.

An execution drain is not a document boundary. Private literal-owner ranges
survive formatting, link resolution and transparent relative-indent scopes;
after execution, a continuous no-fill/literal display becomes one complete
`Preformatted` block. Internal resolved gaps become blank content rows, and
accepted origin changes use existing owner-local row hints. Links, styles and
anchors keep their original annotation trees. Separate displays and semantic
structures retain their boundaries. Unproven leading/trailing gaps and unusual
metadata-only fragments remain separate resolved boundaries, not guessed code
rows. No public continuation flag or frontend stitching pass is required.

After complete roff owners are assembled, the reading projection omits exactly
one empty initial code row when later nonblank body exists. This documented
exception to native terminal output does not trim author spaces, tabs or NBSP,
two-plus initial rows, wholly blank owners, internal/trailing rows, or a literal
prefix sharing a definition HEAD row. Row origins and exact native HP evidence
are rebased before semantic names and positions are identified. Markdown input
is unchanged; consumers render the normalized IR without another trimming pass.

Native tbl rows use a shared execution plan separate from their semantic
cell owners. Every declared execution column enters before any row buffer is
flushed; horizontal-span continuations skip entry, while missing data slots
still clear column-local backtracking. The word executor records actual input
writes before presentation, including invisible cells and blank overstrikes.
Numeric/rule padding and width-dependent horizontal lines use one lazy pinned
width calculation per table, never IR text as a buffer proof. A truly empty
row and a direct whole-row border remain opposite controls. Admitted T{}
recovery publishes its execution receipt atomically; refusal retains native
cell input and cannot publish speculative state. Bounded measurement or column
work refusal preserves every safe owner and following text, with explicit
`manual.table-execution-incomplete` `ContentCoverage` rather than a claimed
successful native flush. No formatter registers enter public IR.

Definition fields record native cells as the text executor decodes them,
before text is styled, linked, or compacted. The pinned CVS `term_fill()`
consumer decides accepted source intervals and rejected suffixes; semantic
output does not reconstruct native graph or buffer occupancy. Stable private
owner markers carry those intervals through inline wrappers and are removed
at the IR drain. A real flush commits its accepted prefix permanently and
ends the rejected field; draining a HEAD owner alone preserves the live
field for BODY execution. A pending glyph, an invisible native graph, a
generated gap, and an already completed physical row are separate facts.

The live field scanner resumes at its unconsumed suffix. Projection offsets
and native blank positions are indexed, so appending words or repeated
word-end breaks does not clone and rescan cumulative field history. Unicode
escaped spaces use the frozen UTF-8 `encode1()` execution path; the direct
buffered blank used for automatic KEEP spacing remains distinct. A generated
HEAD/BODY gap executes its full native count even when its semantic column
was already assigned to HEAD.

Complete corpus regressions live in the repository integration tests, outside
the published `src/**` source set. Packaged unit tests remain self-contained.
Target and topology audits complement these exact text/font/line assertions;
a clean target ledger alone does not establish rendering fidelity.

The authoritative format contracts are [mant-markdown(7)](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-markdown.md), [mant-roff(7)](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-roff.md), and [mant-ir(7)](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-ir.md).

The private roff field buffer in
`src/mandoc/inline/flow/field_buffer.rs` and its `field_buffer/` helpers,
ordinary input retirement in `src/mandoc/inline/flow/plain_input/`,
and the native field device
calculation in `src/mandoc/inline/flow/definition/device/` derive from
the pinned CVS mandoc `term.c`. These translated modules
carry the original ISC copyright and permission notice.
`src/mandoc/inline/flow/tab_stops.rs` adapts the same snapshot's `term_tab.c`
stop-list rules with direct periodic lookup and retains its ISC notice.
The shared display Tab phases and nested list phases in
`src/mandoc/inline/display_tabs.rs` and
`src/mandoc/inline/flow/definition/list_scope.rs` and
`src/mandoc/inline/flow/definition/name_scope.rs` adapt the pinned
`mdoc_term.c`/`man_term.c` handlers and retain their ISC notices.
The shared `Nm` handler in `src/mandoc/inline/names.rs` also adapts
`mdoc_term.c` and retains its ISC notice.
The shared synopsis entry in `src/mandoc/inline/synopsis.rs` and function
field phases in `src/mandoc/inline/flow/definition/function_scope.rs` adapt
the same handlers and retain their ISC notices.
The private row presentation kernel in `src/mandoc/inline/flow/presentation.rs`
and its `presentation/field.rs` helper adapt `roff_term.c` temporary offsets
and `term.c`/`term_ascii.c` field placement and line retirement. Their headers
retain the applicable ISC notices; their public projection is source-neutral IR.
The remaining `ManT` codec code uses the repository license.

