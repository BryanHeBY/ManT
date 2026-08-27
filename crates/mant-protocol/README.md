# mant-protocol

`mant-protocol` is `ManT`'s transport-neutral interaction boundary. It defines
query contracts and projections shared by in-process hosts, CLI JSON, request
JSON, and compact MCP presentation without owning any transport. It owns schema
markers, logical catalog addresses, pagination, outline, excerpt, search,
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
                                     └─> QuerySearch

ScopeQueryRequest ──> host / mant-engine ──> ScopeQueryResponse

CatalogQuery ──> host ──────────────────> DocumentCatalog ──> compact text

local inspection ───────────────────────> DoctorReport
```

| Family | Current discriminator | Purpose |
| --- | --- | --- |
| Process framing | `mant.cli/v0.10` | Advertised by the `mant` executable |
| Request | `mant.request/v0.10` | Closed input accepted by `--request-json` |
| Scope request/result | `mant.scope-request/v0.10`, `mant.scope-query/v0.10` | Bounded multi-document search and explanation |
| Full query | `mant.query/v0.10` | Document plus optional tldr content |
| Document | `mant.document/v0.10` | Versioned projection of the normalized document |
| Catalog | `mant.catalog/v0.10` | Registered Markdown and native-manual discovery |
| Outline, excerpt, search | `mant.outline/v0.10`, `mant.excerpt/v0.10`, `mant.search/v0.10` | Focused query projections |
| Doctor | `mant.doctor/v1` | Read-only local installation diagnostics |

The schemas generated from the Rust types are authoritative. Request schemas
are generated for deserialization so closed-object and default behavior match
what the process accepts; response schemas are generated for serialization.
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
    schema: RequestSchema::V0Dot10,
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

assert_eq!(NATIVE_API_VERSION, "0.10");
assert_eq!(request.schema, RequestSchema::V0Dot10);
let _schema = query_request_json_schema();
```

The native query family follows `ManT`'s pre-stable minor release line: `ManT`
0.10.x uses `v0.10`, and patch releases retain the same wire shape. The former
bare `v1` through `v7` schemas were experimental and are intentionally not
accepted by 0.10. Historical tags preserve those contracts; the first stable
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

`mant-protocol` deliberately reuses the semantic `Block`, `Section`, `Inline`,
`DefinitionIdentity`, `DocumentAddress`, source, metadata, diagnostic, and tldr
types from `mant-ir`. Those types form the wire-bearing semantic subset: a
Serde change to any of them is also a protocol change. CI compares every
generated structural schema with the checked-in v0.10 snapshot, so an accidental
IR representation change fails until compatibility is restored or the
affected protocol discriminator is advanced explicitly. Rustdoc descriptions
and schema titles are excluded from that structural comparison.

Focused excerpt and search results share `OutlineTrail`: ordered compact
ancestors plus one typed terminal node. This keeps full tree-chain rendering
and machine navigation consistent without treating exact explanation as a
text search.

Outline requests use `EntryProjection`: `Summary` is the compact default,
`None` emits section topology only, `All` emits the complete nested semantic
index, and `Kinds` retains selected roles plus their required ancestors. An
optional root selector can focus any projection on one section or entry.
Outline entries keep exact selector aliases separate from authored forms and
can expose evidence-backed value domains.
This supports stateless agent exploration: inspect the compact summary, reuse
a returned stable ID as the next request's root, expand all or selected entry
kinds below it, then read the chosen node without renumbering its path within
that current document projection. Paths remain source-order coordinates and
can move when the source manual changes. A kind filter with no matches returns
an empty node set rather than the unrelated section topology.

Normalized document content is defined separately by
[`mant-ir`](https://crates.io/crates/mant-ir). Parsing, lookup, projection, and
rendering live in [`mant-engine`](https://crates.io/crates/mant-engine).
The complete wire contract is documented by
[`mant-protocol(5)`](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-protocol.md).
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## License

Apache-2.0.
