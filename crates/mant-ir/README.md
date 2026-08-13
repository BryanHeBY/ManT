# mant-ir

`mant-ir` defines `ManT`'s normalized, source-neutral document intermediate
representation. Native man/mdoc syntax and Markdown parser events are lowered
into the same owned tree before projection, search, rendering, or interactive
navigation.

Use this crate when an in-process component needs to inspect, transform, index,
or render a document without depending on the parser that produced it. It does
not perform source lookup, parsing, rendering, filesystem access, or process
protocol handling.

## Model at a glance

```text
ResolvedContent
├── address: DocumentAddress?       registered catalog identity
├── document: Document?             normalized full document
│   ├── meta + diagnostics
│   ├── blocks                      content before the first heading
│   └── sections[]                  recursive heading-backed content
│       └── blocks[]                paragraphs, lists, definitions, tables, …
└── tldr: TldrDocument?             distinct quick-reference channel
```

The important public families are:

| API | Purpose |
| --- | --- |
| `Document`, `Section`, `Block`, `Inline` | Source-neutral content tree |
| `DefinitionIdentity` | Addressable option, command, variable, or environment-variable entry |
| `DocumentAddress`, `MarkdownOrigin` | Exact identity in `ManT`'s catalog rather than a physical path |
| `NodeId`, `OutlinePath`, `TextRange` | Typed local identities and coordinates |
| `DocumentIndex` | Immutable lookup sidecar derived from one document |
| `validate_document` | Shared structural invariant checks |
| `Visit`, `VisitMut` | Exhaustive read-only or mutable traversal |

`DocumentMeta::manual_section` is a native manual category such as `1` or
`3p`. A `Section` is a heading-backed content subtree. The names are kept
deliberately separate so consumers cannot confuse storage lookup with
within-document navigation.

## Basic use

Build indexes and run shared validation after obtaining a document from
`mant-engine` or another trusted producer:

```rust
use mant_ir::{Document, DocumentIndex, validate_document};

fn inspect(document: &Document) {
    let index = DocumentIndex::build(document);
    println!("{} addressable identities", index.iter().count());

    for diagnostic in validate_document(document) {
        eprintln!("{}", diagnostic.message);
    }
}
```

## Stability boundary

This is a typed Rust library contract for trusted in-process components. Its
Serde representation supports projections and tests, but serializing an IR
type directly does not create a stable process protocol. External consumers
should use `mant-protocol`, whose envelopes carry explicit schema identifiers
and compatibility rules.

Versioned CLI JSON and MCP contracts live in
[`mant-protocol`](https://crates.io/crates/mant-protocol); parsing and document
operations live in [`mant-engine`](https://crates.io/crates/mant-engine).
The complete node and stability reference is
[`mant-ir(7)`](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-ir.md).

## License

Apache-2.0.
