# mant-ui

`mant-ui` is the Ratatui frontend component used by the `mant` executable. It
renders `ManT`'s in-memory `mant_ir::ResolvedContent` directly and owns
interactive navigation, search, scrolling, links, menus, mouse input, and
terminal presentation. Its host callbacks use `mant-protocol` DTOs for stable
catalog, search, and cross-document interactions without serializing the IR.

## What this crate provides

- A hierarchy-aware, collapsible Outline tree whose nodes include document
  sections and every semantic entry role: options, commands, variables, and
  environment variables.
- Settled-scroll navigation following and selectable Markdown/mdoc page-local
  references.
- A live, bounded catalog finder that delegates complete-snapshot discovery
  and document loading back to the host.
- Typed Markdown/man reference activation, safe external-URI delegation, and
  bounded back/forward history.
- Confirmed full-document search with active and inactive match highlighting.
- tldr quick-reference and source-document rendering through one layout model.
- Keyboard, mouse, scrollbar, and resizable-pane interaction.
- Width-aware visual text selection plus typed requests for plain-text and
  complete-node Text/Markdown clipboard content.
- A Crossterm lifecycle boundary that restores raw mode and the alternate
  screen after normal exit, setup failure, or panic.
- A static, less-like text pager for terminal-owned catalog output; short and
  redirected results pass through without opening a full-screen interface.
- Public `App` and `DocumentView` layers for callers embedding the frontend in
  an existing Ratatui host.

Command-line parsing and document loading deliberately remain outside this
crate.

## Host boundary

```text
mant host
├─ supplies ResolvedContent ───────────────> App / DocumentView
├─ answers CatalogQuery ──────────────────> live finder
├─ resolves an exact DocumentAddress <──── cross-document activation
├─ decides whether to open HTTP(S)/mailto < external-link request
└─ renders/stores a typed CopyRequest <──── selection or semantic node
```

The UI never scans the filesystem, interprets a source path, downloads data,
or opens a URI by itself. It emits typed requests to its host and keeps
page-local jumps in memory. This makes the same component usable by the
`mant` binary and by another Ratatui application with stricter host policy.

## Basic use

The convenience boundary owns the terminal event loop:

```rust,no_run
let query = mant_engine::query_markdown_text(
    "# Demo\n\n## Overview\n\nHello from ManT.\n",
    Some("demo.md".to_owned()),
)?;

mant_ui::run(&query)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`run` requires an interactive terminal. Callers that already own a Ratatui
event loop can construct `mant_ui::App`, route input through its handlers, and
invoke `App::draw` from their frame callback instead.

Use `run_with_catalog` when cross-document discovery and navigation are
required. Use `run_with_catalog_and_scope` when interactive search must begin
with an already-resolved document set; its first bundle remains the initial
page while catalog discovery stays global. Embedders that provide a system
clipboard use `run_with_catalog_and_scope_and_copy`. Its copy callback receives
a `CopyRequest`: visual selections already contain plain text, while semantic
node requests carry the complete resolved content, stable node selector, and
requested format. The callbacks otherwise receive versioned catalog queries,
exact logical document addresses, and already-classified external URIs;
callback failures return to the UI as notices rather than giving the frontend
hidden authority.

Completing an effective document-text drag emits its plain-text copy request
immediately. A successful callback produces a short-lived, non-modal
confirmation; selection extraction excludes presentation-only tldr panel
borders. Keyboard and Edit-menu actions can copy the retained selection again.

## Platform behavior

The frontend is portable across Linux, macOS, and Windows and does not inspect
the original document source. Callers on every supported platform can provide
normalized man, mdoc, or Markdown queries.

Install [`mant`](https://crates.io/crates/mant) for the complete executable.
This component crate is a library and does not install a second command.
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## License

Apache-2.0.
