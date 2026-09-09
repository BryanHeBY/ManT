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

`cells` borrows complete grapheme clusters and preserves byte boundaries. Its input contract is already-sanitized single-line text. It does not replace source-neutral IR geometry or a frontend's wrapping policy.

Scope report functions consume `ScopeQueryResponse` directly. They preserve the supplied evidence page, document order, failures and coverage; they do not execute a scope query or invent per-document pagination. CLI adapters supply ANSI decoration or terminal-safe Markdown identities. MCP uses the same plain/Markdown reports, then applies its own redaction, hints and character paging without depending on CLI format or error types.

The `tldr` module supplies pure quick-reference layout and semantic span roles. CLI delivery and TUI widgets independently apply colors and terminal behavior; using this layout does not require a TUI backend.

## Features and verification

The default feature set is empty. Normal/build dependencies do not include the loader, query engine, native parser, source updater or terminal backends. A full application may independently enable native parsing in its codec dependency; that feature unification is not needed by this crate.

This crate is Apache-2.0 licensed. See [the workspace architecture](https://github.com/BryanHeBY/ManT/blob/dev/docs/architecture/native-engine.md) for the complete ownership graph.
