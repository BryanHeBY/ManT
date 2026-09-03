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
│   ├── sections[]                  recursive heading-backed content
│   │   └── blocks[]                paragraphs, lists, definitions, tables, …
│   └── DefinitionIdentity?         semantic fact attached to a definition
│       └─> SemanticIndex           rebuildable entry hierarchy
└── tldr: TldrDocument?             distinct quick-reference channel
```

The important public families are:

| API | Purpose |
| --- | --- |
| `Document`, `Section`, `Block`, `Inline` | Source-neutral content tree |
| `DefinitionIdentity` | Addressable content definition and exact aliases |
| `SemanticIndex`, `SemanticEntry`, `EntrySummary` | Rebuildable role-aware hierarchy, authored forms, and compact coverage |
| `DocumentAddress`, `MarkdownOrigin` | Exact identity in `ManT`'s catalog rather than a physical path |
| `NodeId`, `FragmentAlias`, `OutlinePath`, `TextRange` | Normalized local identities, exact source fragments, and coordinates |
| `DocumentIndex` | Immutable lookup sidecar derived from one document |
| `validate_document` | Shared structural invariant checks |
| `Visit`, `VisitMut` | Exhaustive read-only or mutable traversal |

`DocumentMeta::manual_section` is a native manual category such as `1` or
`3p`. A `Section` is a heading-backed content subtree. The names are kept
deliberately separate so consumers cannot confuse storage lookup with
within-document navigation.

## Content and semantic indexes

`DocumentIndex` addresses content nodes. `SemanticIndex` separately groups
identified definitions into commands, parameter families, configuration keys,
variables, values, and terms. A semantic entry keeps exact selector aliases,
complete authored forms, content targets, and nested ownership; it does not
replace the definitions in the document tree.

Build both indexes and run shared validation after obtaining a document from
`mant-engine` or another trusted producer:

```rust
use mant_ir::{Document, DocumentIndex, SemanticIndex, validate_document};

fn inspect(document: &Document) {
    let content = DocumentIndex::build(document);
    let semantics = SemanticIndex::build(document);
    println!("{} addressable identities", content.iter().count());

    for entry in semantics.root() {
        println!("{}: {:?}", entry.id, entry.aliases);
        for target in &entry.targets {
            assert!(content.get(target).is_some());
        }
    }

    for diagnostic in validate_document(document) {
        eprintln!("{}", diagnostic.message);
    }
}
```

Entries owned by a heading are available through `SemanticIndex::section`.
Nested entries remain under their parent `SemanticEntry`; callers should not
flatten that hierarchy when ownership affects interpretation.

## Typed links and the document graph

`Inline::Link` carries a closed `LinkTarget` rather than an unclassified URL
string. `Section` targets stay inside the current document. `Document` and
`Manual` targets are logical cross-document edges that a resolver may follow
under explicit bounds. `External` and `Email` targets are host actions and must
not expand a documentation scope.

The IR intentionally does not resolve those edges. A `Document` target needs
the referring `DocumentAddress` so `mant-engine` can keep relative links inside
their registered source; a `Manual` target still requires catalog lookup and
explicit ambiguity handling. Renderers that cannot activate a target should
preserve the link's visible children.

`NodeId` is always the normalized internal identity used by indexes and typed
local links. A document root, section, or inline anchor may additionally carry
exact `FragmentAlias` values contributed by source syntax such as mdoc `.Tg`
or a Markdown heading ID. Those aliases preserve external deep links without
weakening the normalized-ID invariant. `DocumentIndex::fragment_target`
resolves either form only when it identifies one canonical target.

## Stability boundary

This is a typed Rust library contract for trusted in-process components. Its
Serde representation supports projections and tests, but serializing an IR
type directly does not create a stable process protocol. External consumers
should use `mant-protocol`, whose envelopes carry explicit schema identifiers
and compatibility rules.

Versioned CLI JSON contracts and compact MCP query projections live in
[`mant-protocol`](https://crates.io/crates/mant-protocol); parsing and document
operations live in [`mant-engine`](https://crates.io/crates/mant-engine).
The complete node and stability reference is
[`mant-ir(7)`](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-ir.md).
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## License

Apache-2.0.
