# mant-core

`mant-core` is ManT's renderer-independent document engine. It resolves local
documents through `mant-sources` into the contracts from `mant-ast`, builds
semantic projections, and produces deterministic output without owning a
terminal or command-line process.

## What this crate provides

- Registered Markdown lookup through the read-only `mant-sources` boundary.
- A conservative, source-positioned Markdown parser with explicit loss
  diagnostics and optional embedded tldr content.
- Bounded native manual loading, explicit leaf-file symlink support,
  root-constrained `.so` alias resolution, and `man(7)`/`mdoc(7)` lowering on
  every supported platform.
- Semantic outlines containing addressable sections and role-aware entries.
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
let outline = build_outline_with_detail(&query, OutlineDetail::Entries)?;

println!("{}", render_outline_text(&outline));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `resolve_query` or `resolve_query_with_policy` when a caller needs the full
document bundle. Use `execute_query` to validate, resolve, and materialize the
request's `view` through one engine boundary. Use `parse_markdown` when the
caller needs the parsed document and tldr preface without query composition.
`DocumentResolver` can be reused when several operations must share one lazy
filesystem snapshot; constructing a new resolver refreshes discovery.

## Platform behavior

| Platform | Markdown engine | Native man/mdoc engine |
| --- | --- | --- |
| Linux with glibc | Yes | Bundled `libmandoc-rs` |
| macOS | Yes | Bundled `libmandoc-rs` |
| Windows | Yes | Bundled `libmandoc-rs` |

Every supported target compiles `libmandoc-rs`. Windows uses its memory-only C
transport while Rust owns file I/O, decompression, paths, and `.so` redirects.

## Layering

`mant-core` returns owned `mant-ast` values and does not expose libmandoc C
structures. Applications that only need raw roff syntax should use
[`libmandoc-rs`](https://crates.io/crates/libmandoc-rs) directly. Applications
that need the complete command or reader should install
[`mant`](https://crates.io/crates/mant).

Architecture and source-resolution details are documented in the
[ManT native-core reference](https://github.com/BryanHeBY/ManT/blob/main/docs/architecture/native-core.md).

## License

Apache-2.0. Native builds also contain the separately attributed vendored mandoc
sources supplied by `libmandoc-rs`.
