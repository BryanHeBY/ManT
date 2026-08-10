# mant-ast

`mant-ast` defines ManT's versioned, renderer-neutral document and query
contracts. The native engine, structured CLI, MCP server, and Ratatui frontend
share these exact Rust types instead of translating between frontend-specific
models.

## What this crate provides

- A source-neutral document AST for prose, sections, definitions, lists,
  tables, code, equations, links, layout hints, and diagnostics.
- Closed request and response contracts for complete queries, outlines,
  excerpts, semantic search, and optional tldr content.
- Explicit schema markers and stable IDs suitable for process boundaries.
- Draft 2020-12 JSON Schema generation from the authoritative Rust types.
- Serde serialization without parser, filesystem, terminal, or process
  dependencies.

The crate intentionally contains no source lookup, parsing, projection, or
rendering logic. Those operations live in
[`mant-core`](https://crates.io/crates/mant-core).

## Basic use

```rust
use mant_ast::{
    OutlineDetail, QueryInput, QueryRequest, QueryView, RequestSchema,
    query_json_schema_catalog,
};

let request = QueryRequest {
    schema: RequestSchema::V5,
    input: QueryInput::Document {
        name: "git".to_owned(),
        source: None,
        section: None,
    },
    view: QueryView::Outline {
        detail: OutlineDetail::Options,
    },
};

assert_eq!(request.schema, RequestSchema::V5);
assert!(query_json_schema_catalog().contains_key("request"));
```

Consumers should match exact schema variants rather than inferring
compatibility from the crate's semantic version. The native API version is
also available as `NATIVE_API_VERSION`.

## Related crates

- `mant-core` loads and lowers source documents into these contracts.
- `mant-ui` renders `QueryBundle` values interactively.
- `mant` exposes the contracts through CLI JSON, generated schemas, and MCP.

The complete protocol reference and JSON examples live in the
[ManT repository](https://github.com/BryanHeBY/ManT/blob/main/docs/protocol.md).

## License

Apache-2.0.
