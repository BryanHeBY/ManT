# Migrating from `libmandoc-rs` to `mantdoc`

`mantdoc` is the behavioral replacement for `libmandoc-rs`, not a
source-compatible rename. It keeps the parser semantics and owned-AST fields
that ManT needs, while deliberately replacing the recursive public tree and
ambient I/O with a bounded arena and explicit authority.

This guide describes the public contract currently available in the unpublished
`0.1.0-alpha` workspace crate. The native M7 parser, M8 engine integration,
and M9 renderer parity gates are complete; the remaining release work is
tracked in the [migration plan](architecture/mantdoc-migration.md).

## Core parse

The legacy parser accepted a path-like name alongside byte input. The native
core makes the logical identity explicit and never performs filesystem I/O:

```rust
use mantdoc::{Parser, ParserConfig, Source, SourceName, Syntax};

let name = SourceName::new("man1/widget.1")?;
let parser = Parser::new(ParserConfig {
    syntax: Syntax::Man,
    operating_system: Some("ExampleOS".into()),
    ..ParserConfig::default()
});
let report = parser.parse(Source::new(&name, b".TH WIDGET 1\n.SH NAME\nwidget\n"))?;
```

`ParserConfig::syntax` replaces `InputFormat`; `operating_system` replaces
`with_mdoc_operating_system`. A source-authored `.Os` wins over the configured
fallback. `ParseReport` contains a bounded `Document`, typed `Diagnostic`
values, and `ParseStatistics`; normal syntax recovery is reported in the
report, while invalid configuration and hard input boundaries return
`FatalError`.

## Traversing the AST

`libmandoc-rs::Node { children: Vec<Node> }` becomes an immutable arena with
opaque node IDs. Traverse through `Document::root`, `Document::node`, child
iterators, or `Document::preorder`; no storage index is public:

```rust
for node in report.document.preorder() {
    if let Some(text) = node.text() {
        println!("{:?}: {text}", node.kind());
    }
}
```

`NodeRef` exposes the legacy lowering-relevant data: roles, macro names,
normalized text/tag/equation payloads, source spans, flags, list/display/font/
author/enclosure state, layout strings, and table cells. A span's `SourceId`
is document-local; resolve it with `Document::source_name` and
`Document::source_position` before presenting it outside the process.

`SpecialCharacter` and `special_character(name)` replace the legacy named-roff
character helper. The native 346-entry pinned mandoc 1.14.6 catalog returns
either `Visible(char)` or `ZeroWidth`; an unknown spelling returns `None`.

The default 256-level tree and equation boundaries intentionally preserve the
old wrapper's finite-prefix behavior. They emit typed
`legacy.syntax-tree-depth-limit` and `legacy.equation-tree-depth-limit`
diagnostics. If a caller chooses a narrower `Limits` value, the result instead
uses native `limits.tree-depth` or `limits.equation-depth` diagnostics.

## Files, compression, and includes

Transport is outside the byte-only core. `parse_bytes` and `parse_file` take a
logical `SourceName` plus explicit `Compression`; `Auto` detects gzip/zstd
frame magic, never a filename suffix:

```rust
use mantdoc::{Compression, Parser, SourceName};

let name = SourceName::new("widget.1")?;
let report = Parser::default().parse_file(&name, "./widget.1.gz", Compression::Auto)?;
```

Enable the additive `gzip` and/or `zstd` Cargo features for those transports.
Both compressed input and its decoded root are limited by
`Limits::max_root_source_bytes`; there is no old Unix/Windows gzip split.

`parse_file` does not authorize `.so` filesystem lookup. For a trusted virtual
tree use `SourceBundle` plus `parse_bundle`; for a contained filesystem tree
use `parse_file_in_root(root, path, compression)`. The latter derives a
canonical root-relative logical name and rejects absolute, escaping, and
symlink-outside includes. For a different authority model, implement
`SourceResolver` and call `parse_with_resolver`; the parser never falls back to
the process working directory.

## Diagnostics and serialization

`DiagnosticCode` is a stored, validated value rather than a classification
inferred from the diagnostic message. Diagnostics have a `Severity`, optional
primary span, optional related spans, and a message. New consumers should branch
on `code` and not message wording.

With the optional `serde` feature, serialize `LogicalParseReport::from(&report)`
rather than arena internals. Schema version 1 contains sibling-index AST paths,
typed diagnostics, logical source names plus byte offsets/line/column, and
parse counters. It intentionally cannot deserialize back into a parser session
or a `Document`; retain the original source if a later reparse is required.

## Renderers

The optional `render` feature provides bounded native ASCII, UTF-8, and HTML
reference renderers. It shares the same explicit source and transport policy as
the parser core, and the renderer fuzz target exercises this native surface.
