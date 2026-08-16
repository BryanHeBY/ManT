# mant-protocol

`mant-protocol` is `ManT`'s versioned structured interaction boundary. It
defines the request and response DTOs shared by in-process hosts, CLI JSON,
request JSON, and MCP without owning any transport. It owns schema markers,
logical catalog addresses, pagination, outline, excerpt, search, tldr-update
results, local doctor reports, and JSON Schema generation. The `mant` crate
separately composes host callbacks, process framing, and MCP transport.

Use this crate whenever a Rust host or process consumer needs stable structured
inputs and projections. The same DTO may cross an in-memory callback or a
serialized transport; serialization is a supported representation, not the
crate's sole purpose. It contains data contracts only: it performs no document
discovery, parsing, query execution, rendering, terminal I/O, or MCP transport.

## Contract families

```text
QueryRequest ──> host / mant-engine ──┬─> QueryBundle
                                     ├─> QueryOutline
                                     ├─> QueryExcerpt
                                     └─> QuerySearch

CatalogQuery ──> host ──────────────────> DocumentCatalog

local inspection ───────────────────────> DoctorReport
```

| Family | Current discriminator | Purpose |
| --- | --- | --- |
| Process framing | `mant.cli/v0.8` | Advertised by the `mant` executable |
| Request | `mant.request/v0.8` | Closed input accepted by `--request-json` |
| Full query | `mant.query/v0.8` | Document plus optional tldr content |
| Document | `mant.document/v0.8` | Versioned projection of the normalized document |
| Catalog | `mant.catalog/v0.8` | Registered Markdown and native-manual discovery |
| Outline, excerpt, search | `mant.outline/v0.8`, `mant.excerpt/v0.8`, `mant.search/v0.8` | Focused query projections |
| Doctor | `mant.doctor/v1` | Read-only local installation diagnostics |

The schemas generated from the Rust types are authoritative. Request schemas
are generated for deserialization so closed-object and default behavior match
what the process accepts; response schemas are generated for serialization.

## Basic use

Construct requests with the typed tagged unions and discover the exact JSON
Schema rather than copying a shape by hand:

```rust
use mant_protocol::{
    NATIVE_API_VERSION, OutlineDetail, QueryInput, QueryRequest, QueryView,
    RequestSchema, query_request_json_schema,
};

let request = QueryRequest {
    schema: RequestSchema::V0Dot8,
    input: QueryInput::Document {
        selector: "git".to_owned(),
        source: None,
        manual_section: None,
    },
    view: QueryView::Outline {
        detail: OutlineDetail::Entries,
    },
};

assert_eq!(NATIVE_API_VERSION, "0.8");
assert_eq!(request.schema, RequestSchema::V0Dot8);
let _schema = query_request_json_schema();
```

The native query family follows `ManT`'s pre-stable minor release line: `ManT`
0.8.x uses `v0.8`, and patch releases retain the same wire shape. The former
bare `v1` through `v7` schemas were experimental and are intentionally not
accepted by 0.8. Historical tags preserve those contracts; the first stable
native protocol will use a `v1.0` release line. Independent contracts such as
`mant.doctor/v1` and `mant.markdown/v1` keep their own identifiers. Clients
must therefore compare complete discriminators.

Adding or changing a Rust field does not by itself authorize a wire change.
The native discriminator must advance whenever its serialized contract
changes outside a patch-compatible addition.

`mant-protocol` deliberately reuses the semantic `Block`, `Section`, `Inline`,
`DefinitionIdentity`, `DocumentAddress`, source, metadata, diagnostic, and tldr
types from `mant-ir`. Those types form the wire-bearing semantic subset: a
Serde change to any of them is also a protocol change. CI compares every
generated structural schema with the checked-in v0.8 snapshot, so an accidental
IR representation change fails until compatibility is restored or the
affected protocol discriminator is advanced explicitly. Rustdoc descriptions
and schema titles are excluded from that structural comparison.

Focused excerpt and search results share `OutlineTrail`: ordered compact
ancestors plus one typed terminal node. This keeps full tree-chain rendering
and machine navigation consistent without treating exact explanation as a
text search.

Normalized document content is defined separately by
[`mant-ir`](https://crates.io/crates/mant-ir). Parsing, lookup, projection, and
rendering live in [`mant-engine`](https://crates.io/crates/mant-engine).
The complete wire contract is documented by
[`mant-protocol(5)`](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-protocol.md).

## License

Apache-2.0.
