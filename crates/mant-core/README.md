# mant-core

`mant-core` is ManT's renderer-independent document engine. It resolves local
sources into the contracts from `mant-ast`, builds semantic projections, and
produces deterministic output without owning a terminal or command-line
process.

## What this crate provides

- Registered Markdown discovery under native platform data directories.
- A conservative, source-positioned Markdown parser with explicit loss
  diagnostics and optional embedded tldr content.
- Bounded native manual loading, constrained `.so` alias resolution, and
  `man(7)`/`mdoc(7)` lowering on Unix.
- Semantic outlines containing addressable sections and command options.
- Excerpt selection and literal or regular-expression search with generated
  Markdown coordinates.
- Markdown, text, man-style text, and JSON renderers over one normalized AST.
- Installed-client and private tldr cache discovery and updates.

Process argument parsing, MCP transport, and interactive presentation remain
outside this crate.

## Basic use

The in-memory Markdown path is deterministic and works on every supported
platform:

```rust
use mant_ast::OutlineDetail;
use mant_core::{
    build_outline_with_detail, query_markdown_text, render_outline_text,
};

let query = query_markdown_text(
    "# Demo\n\n## Options\n\n- `--verbose`: Show more detail.\n",
    Some("demo.md".to_owned()),
)?;
let outline = build_outline_with_detail(&query, OutlineDetail::Options)?;

println!("{}", render_outline_text(&outline));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `query` or `query_with_policy` with a `mant_ast::QueryRequest` to resolve a
registered document or local manual by name. Use `parse_markdown` when the
caller needs the parsed document and tldr preface without query composition.

## Platform behavior

| Platform | Markdown engine | Native man/mdoc engine |
| --- | --- | --- |
| Linux with glibc | Yes | Bundled `libmandoc-rs` |
| macOS | Yes | Bundled `libmandoc-rs` |
| Windows | Yes | Not available |

Unix builds compile the target-specific `libmandoc-rs` dependency. Windows
omits it entirely while retaining registered documents, parsing, projections,
search, output, tldr, and the same versioned contracts.

## Layering

`mant-core` returns owned `mant-ast` values and does not expose libmandoc C
structures. Applications that only need raw roff syntax should use
[`libmandoc-rs`](https://crates.io/crates/libmandoc-rs) directly. Applications
that need the complete command or reader should install
[`mant`](https://crates.io/crates/mant).

Architecture and source-resolution details are documented in the
[ManT native-core reference](https://github.com/BryanHeBY/ManT/blob/main/docs/architecture/native-core.md).

## License

Apache-2.0. Unix builds also contain the separately attributed vendored mandoc
sources supplied by `libmandoc-rs`.
