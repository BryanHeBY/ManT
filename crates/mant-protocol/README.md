# mant-protocol

`mant-protocol` is `ManT`'s transport-neutral interaction boundary. It defines
query contracts and projections shared by in-process hosts, CLI JSON, request
JSON, and compact MCP presentation without owning any transport. It owns schema
markers, logical catalog addresses, pagination, outline, excerpt, explanation, search,
tldr-update results, local doctor reports, deterministic catalog presentation,
and JSON Schema generation. The `mant` crate separately composes host
callbacks, process framing, terminal policy, and MCP transport.

Use this crate whenever a Rust host or process consumer needs stable inputs,
projections, or deterministic non-terminal presentation. The same DTO may
cross an in-memory callback, be serialized by a versioned JSON boundary, or be
rendered into a compact MCP result; serialization is a supported
representation, not the crate's sole purpose. It performs no document
discovery, parsing, query execution, terminal I/O, or MCP transport.

## Contract families

```text
QueryRequest ──> host / mant-engine ──┬─> QueryBundle
                                      ├─> QueryOutline
                                      ├─> QueryExcerpt
                                      ├─> QueryExplanation
                                      └─> QuerySearch

ScopeQueryRequest ──> host / mant-engine ──> ScopeQueryResponse

CatalogQuery ──> host ──────────────────> DocumentCatalog ──> compact text

local inspection ───────────────────────> DoctorReport
explicit cache maintenance ─────────────> TldrCacheUpdate
```

| Family | Current discriminator | Purpose |
| --- | --- | --- |
| Process framing | `mant.cli/v0.11` | Advertised by the `mant` executable |
| Request | `mant.request/v0.11` | Closed input accepted by `--request-json` |
| Scope request/result | `mant.scope-request/v0.11`, `mant.scope-query/v0.11` | Bounded multi-document search and explanation |
| Full query | `mant.query/v0.11` | Document plus optional tldr content |
| Document | `mant.document/v0.11` | Versioned projection of the normalized document |
| Catalog | `mant.catalog/v0.11` | Registered Markdown and native-manual discovery |
| Outline, excerpt, search | `mant.outline/v0.11`, `mant.excerpt/v0.11`, `mant.search/v0.11` | Focused query projections |
| Explanation | `mant.explanation/v0.11` | Bounded independent name/form/content/relationship evidence |
| Doctor | `mant.doctor/v1` | Read-only local installation diagnostics |
| tldr update | `mant.tldr-update/v1` | Explicit native cache-maintenance result |

The schemas generated from the Rust types are authoritative. Request schemas
are generated for deserialization so closed-object and default behavior match
what the process accepts; response schemas are generated for serialization.
`mant --schema all` includes every row above, including the two independent
maintenance and diagnostic contracts.
These native schema discriminators describe CLI and request JSON. MCP uses its
own negotiated protocol version and presents the same logical identities and
focused projections as bounded text or `CommonMark` instead of serializing the
native response envelopes.

## Basic use

Construct requests with the typed tagged unions and discover the exact JSON
Schema rather than copying a shape by hand:

```rust
use mant_protocol::{
    EntryProjection, NATIVE_API_VERSION, QueryInput, QueryRequest, QueryView,
    RequestSchema, query_request_json_schema,
};

let request = QueryRequest {
    schema: RequestSchema::V0Dot11,
    input: QueryInput::Document {
        selector: "git".to_owned(),
        source: None,
        manual_section: None,
    },
    view: QueryView::Outline {
        entries: EntryProjection::Summary,
        root: None,
    },
};

assert_eq!(NATIVE_API_VERSION, "0.11");
assert_eq!(request.schema, RequestSchema::V0Dot11);
let _schema = query_request_json_schema();
```

The native query family follows `ManT`'s pre-stable minor release line: `ManT`
0.11.x uses `v0.11`, and patch releases remain backward compatible. They may
add documented optional response fields, but never change requests, required
fields, tagged unions, or existing field semantics. The former
bare `v1` through `v7` schemas were experimental and are intentionally not
accepted by 0.11. Historical tags preserve those contracts; the first stable
native protocol will use a `v1.0` release line. Independent contracts such as
`mant.doctor/v1` and `mant.markdown/v1` keep their own identifiers. Clients
must therefore compare complete discriminators. The `mant-protocol` crate has
its own semver; upgrading that Rust package does not by itself select a new
wire discriminator.

Adding or changing a Rust field does not by itself authorize a wire change.
The native discriminator must advance whenever its serialized contract
changes outside a patch-compatible addition.

Scoped search has one pagination coordinate system. `ScopeSearch` owns the
global total, offset, truncation flag, and continuation offset; each
`ScopedSearchDocument` carries only its logical address, depth, canonical
Markdown render descriptor, and globally numbered hits. Consumers must never
derive a continuation cursor from an individual document group.

Explanation is a separate `QueryExplanation` contract, not an excerpt wrapper.
`ExplanationQuery` supplies a literal and bounded `ExplanationOptions` (50
owners by default, at most 256; zero-based offset; 1 MiB default / 4 MiB maximum
forms/facts/previews/body copy budget). `EvidenceClass` distinguishes direct
entries, explicitly related entries, entry mentions and context mentions.
One owner retains all match bases but only one class, independent of omitted
details or empty names. `order` is always `class-then-source`; the four fixed
`counts` totals/returned sum to the response counts, including zero categories.
`ScopeExplanation` has one globally ordered `evidence` page, one cursor and
one copy budget. Its BFS `documents` are source reports without nested bodies
or cursors; each record's `documentIndex` refers to those reports, not the
outer scope graph. Class priority precedes document order even at limit 1.

Every record has `previews` and `previewsOmitted`. Literal matches may retain
two distinct matched blocks in source order, each a window of at most 1024
Unicode scalars preserving a complete match. Ranges are half-open scalar
positions after control masking; `source` and absolute final-IR `blockPath`
identify the actual block. Paths start at `root` or `sections/sN[/sN...]`, with
`bN`, `iN`, `dN` and `rN/cN` components. Facts, windows and complete body use the
same budget, in that order. Clipping is not omission; an omitted window sets
`previewsOmitted` and content truncation, never replaces atomic `content`. Normal multiple results and no-evidence are not failures;
check `outcome`, source coverage, truncation and diagnostics separately.
`semanticsComplete` is validation coverage, not exhaustive recall.

An independently declared empty definition is a real direct record, not a
missing-body result: it must not borrow the following owner's description.
Renderers may suppress Forms only when that particular record displays the
complete matching owner and forms. Mentions and omitted/cropped content retain
the separately returned forms. Plain reports have no generated body frame;
their headings aid navigation but do not authenticate untrusted body text.
Structured consumers use the DTO's class, source and omission fields instead.

Name/Form bases retain bounded matched spellings and occurrences, not just a
unit discriminator; Identity bases identify the matched outline fields.
`ExplanationEntry::name_bindings` is the independent, owner-local ordinary-name
projection. Form and content locations use half-open safe-text Unicode scalars,
with typed definition-term roots and response-relative paths. Consumers can
resolve them using `ExplanationContentRange::resolve` and
`ExplanationFormRange::resolve` after deserialization, without the source document.
Invalid locations are ignored rather than rediscovered by text searching.
The separate `match_details_omitted` and `name_bindings_omitted` flags report
bounded detail loss, independently of complete body/metadata omission. Limits
are 32 match records, 32 ordinary bindings, 32 occurrences, 32 fragments per
occurrence/domain and 1,024 fragments per owner, all within the copy budget.

`mant-protocol` deliberately reuses the semantic `Block`, `Section`, `Inline`,
`EntryFacts`, `DocumentAddress`, source, metadata, diagnostic, and tldr
types from `mant-ir`. Those types form the wire-bearing semantic subset: a
Serde change to any of them is also a protocol change. CI compares every
generated structural schema with the checked-in v0.11 snapshot, so an accidental
IR representation change fails until compatibility is restored or the
affected protocol discriminator is advanced explicitly. Rustdoc descriptions
and schema titles are excluded from that structural comparison.

Focused excerpt, explanation and search results share `OutlineTrail`: ordered compact
ancestors plus one typed terminal node. This keeps full tree-chain rendering
and machine navigation consistent. Ordinary evidence adds its real IR block
path, without manufacturing an entry or replacing strict selection.

Outline requests use `EntryProjection`: `Summary` is the compact default,
`None` emits section topology only, `All` emits the complete nested semantic
index, and `Kinds` retains selected roles plus their required ancestors. An
optional root selector can focus any projection on one section or entry.
Outline entries keep exact selector `names` separate from authored
forms, explicit `aliasGroups` / `aliasOf`, and evidence-backed value domains.

Definitions remain authoritative content in `mant-ir`; `SemanticEntry` is a
rebuildable concept index, and an outline entry is the selected protocol
projection. Its `id` selects the definition that supplies content, while
`documentTargets` describes explicit links carried by entry terms.
This separation lets summary and role-filtered outlines omit entries without
changing the document or inventing another semantic model.

This supports stateless agent exploration: inspect the compact summary, reuse
a path or ID from that current response as the next request's root, expand all
or selected entry kinds below it, then read the chosen node. Exact path and ID
resolution precedes aliases and shorthands in strict navigation. Paths remain
source-order coordinates; clients rediscover after the source manual changes.
A kind filter with no matches returns an empty node set rather than the
unrelated section topology.

Normalized document content is defined separately by
[`mant-ir`](https://crates.io/crates/mant-ir). Parsing, lookup, projection, and
rendering live in [`mant-engine`](https://crates.io/crates/mant-engine).
The complete wire contract is documented by
[`mant-protocol(5)`](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-protocol.md).
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## In-memory presentation

`EntryLabelMode` makes Compact (validated names, visible forms, then ID) and
Forms (visible forms first) explicit. `EntryTone` preserves the complete
`EntryKind` at adapter boundaries; terms are primary content, not muted metadata.
These are display policies, not aliases, confidence levels or new wire fields.

`EntryStyleMap` prepares owner-local validated name ranges once per borrowed
document or excerpt. `project_content_slice` maps UTF-8 content slices into
root-relative Unicode scalar ranges without copying the source tree.
`visit_inline_text` composes those ranges with Strong, Emphasis, Code and Link
markup as borrowed spans. Adapters own colors, escaping and line geometry;
they must not reconstruct name bindings by searching rendered strings.
Query-match and selection overlays remain separate from ordinary name roles.
The shared [entry presentation contract](https://github.com/BryanHeBY/ManT/blob/main/docs/architecture/entry-presentation.md)
documents ownership, coordinate roots and adapter responsibilities.

## License

Apache-2.0.
