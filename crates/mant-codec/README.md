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
| Inline and physical lines | `InlineBuilder` executes word/control events; `source_cursor.rs` places source-visible events on physical lines. No-fill changes layout, not macro interpretation. |
| Structural layout | Block drivers own pending paragraphs and list state; section, synopsis, man no-fill and dialect-specific list consumers remain separate. Shared definition helpers do not own formatter state. |
| Speculative recovery | Table-cell candidates retain output, final formatter state and diagnostics until ownership acceptance. Rejection rolls back all three, unlike a normal font-scope exit. |

Source geometry retains bounded basic-unit positions independently of the IR
parent and man macro base. Conversion produces signed relative offsets;
ownership normalization rebases only transferred roots. Text presentation
composes each parent once. It keeps resolved gap requests separate from literal
newlines through nested flow composition, sharing cell/marker/gap rules with
the UI through `mant-ir::geometry`. Zero block spacing is tight, not a
frontend default; each source request has one consumption point.

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
`src/mandoc/inline/flow/field_buffer.rs` and the native field device
calculation in `src/mandoc/inline/flow/definition/device.rs` derive from
the pinned CVS mandoc `term.c`. These translated modules
carry the original ISC copyright and permission notice.
`src/mandoc/inline/flow/tab_stops.rs` adapts the same snapshot's `term_tab.c`
stop-list rules with direct periodic lookup and retains its ISC notice.
The shared display Tab phases and nested list phases in
`src/mandoc/inline/display_tabs.rs` and
`src/mandoc/inline/flow/definition/list_scope.rs` adapt the pinned
`mdoc_term.c`/`man_term.c` handlers and retain their ISC notices.
The remaining `ManT` codec code uses the repository license.
