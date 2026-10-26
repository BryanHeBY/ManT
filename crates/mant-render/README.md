# mant-render

Deterministic presentation of existing `ManT` documents and query results. This crate renders IR content and protocol DTOs; it does not load documents, run queries, open links, or control a terminal.

## Boundaries

- `mant-ir` owns document content, source layout facts, logical targets and URI encoding.
- `mant-protocol` owns wire contracts, validation and stable semantic labels.
- `mant-codec` owns document Markdown encoding. Rendered excerpt and outline reports compose that encoder without enabling its native roff backend.
- `mant-render` owns text and report formatting, semantic presentation roles, reference labels, span decoration and borrowed grapheme-safe cell primitives.
- CLI and TUI adapters map abstract roles to ANSI or Ratatui styles. The UI retains wrapping policy, viewport state, hit maps and selection geometry.

All public rendering functions receive already prepared IR or DTO values. Plain reports are deterministic and contain no terminal escape sequences. Decorated variants let the adapter supply a role-to-text callback; they do not select a theme or write process output.

```rust
use mant_ir::ResolvedContent;
use mant_render::{render_query_text, render_query_text_with};

let content = ResolvedContent {
    address: None,
    label: "demo".into(),
    document: None,
    tldr: None,
};
let plain = render_query_text(&content);
assert_eq!(plain, render_query_text_with(&content, |_, text| text.to_owned()));
```

`render_query_text_at_width` and `render_query_man_at_width` apply row alignment
within a caller-supplied content width, bounded at 4096 cells. They preserve the
accepted hard rows; ordinary document rows do not acquire terminal soft wrapping.
Table prose may wrap inside responsive column slots before a table falls back
to source order. A soft seam consumes its internal breakable separator without
adding an empty visual row or charging that separator to cell alignment.
Authored edge whitespace, hard rows and nonbreaking spaces remain distinct;
ANSI and OSC wrappers close and reopen around each painted slice. Unbounded text
and report functions use left alignment while retaining resolved row origins;
no rendering API queries terminal dimensions or changes authoritative text.

Table `alignment` positions a content-sized frame at a known width; omission
keeps automatic flow. Cell alignment uses its column slot, while a non-left
owner-row hint can independently align within its final receiving area. One
bounded table plan supplies data and all rule endpoints, including after a
source-order fallback. `column_count` retains unoccupied trailing columns
without adding cell owners. Pipe separators belong to this text projection,
so its frame width can differ from TUI layout. Empty physical rows, navigation
and authored spaces survive either plan.

Author-requested table frames and vertical edges use the chosen column plan,
not the unwrapped intrinsic width. Soft cell widths remain separate from
sequential declared fields. Declared-field tables retain their outside frame
but may simplify interior grid lines. Horizontal spans receive their full covered area;
frame/rule/grid glyphs are generated presentation and never replace cell text.
Source-order stacking keeps automatic horizontal grids at original data-row
boundaries, including tables without an outside frame. Explicit rules supply
their boundary once; a fully continuing row span suppresses the automatic line
within the bounded column plan. Sparse over-budget spans retain the simplified
stacked geometry rather than allocating a slot per declared column.

`table_border` supplies the terminal stroke glyphs shared by CLI and TUI:
`horizontal_glyph`, `vertical_glyph`, `crossing`, `junction` and `frame_corner`.
Single/double stroke strengths come from IR, but glyph selection stays in this
presentation layer. These helpers neither allocate columns nor choose colors;
adapters retain their own layout, theme and source-coordinate maps.

`cells` borrows complete grapheme clusters and preserves byte boundaries. Its input contract is already-sanitized single-line text. It does not replace source-neutral IR geometry or a frontend's wrapping policy.

Scope report functions consume `ScopeQueryResponse` directly. They preserve the supplied evidence page, document order, failures and coverage; they do not execute a scope query or invent per-document pagination. CLI adapters supply ANSI decoration or terminal-safe Markdown identities. MCP uses the same plain/Markdown reports, then applies its own redaction, hints and character paging without depending on CLI format or error types.

The `tldr` module supplies pure quick-reference layout and semantic span roles. CLI delivery and TUI widgets independently apply colors and terminal behavior; using this layout does not require a TUI backend.

## Features and verification

The default feature set is empty. Normal/build dependencies do not include the loader, query engine, native parser, source updater or terminal backends. A full application may independently enable native parsing in its codec dependency; that feature unification is not needed by this crate.

This crate is Apache-2.0 licensed. See [the workspace architecture](https://github.com/BryanHeBY/ManT/blob/main/docs/architecture/native-engine.md) for the complete ownership graph.
