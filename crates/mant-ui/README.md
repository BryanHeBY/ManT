# mant-ui

The less-like static pager embeds the pinned minus 5.7.2 static/search engine
privately, with a physical-row SGR restoration adapter. The implementation and
complete upstream MIT/Apache-2.0 licenses are packaged under `src/pager/vendor`;
the source inventory and reproducible adaptation are documented there. This
keeps the fix effective for crates.io consumers without exposing another API.

`mant-ui` is the Ratatui frontend component used by the `mant` executable. It
renders `ManT`'s in-memory `mant_ir::ResolvedContent` directly and owns
interactive navigation, search, scrolling, links, menus, mouse input, and
terminal presentation. Its host callbacks use `mant-protocol` DTOs for stable
catalog, search, and cross-document interactions without serializing the IR.

## What this crate provides

- A hierarchy-aware, collapsible Outline tree whose complete group labels
  summarize direct entries, descendants, and forms, then expand into commands, parameters,
  configuration keys, variables, values, and generic terms. Compact entry
  names preserve browsing density while an optional full-label mode wraps
  every authored form. Row-topology changes retain the selected node's viewport
  position whenever terminal bounds permit.
- Settled-scroll navigation following and selectable Markdown/mdoc page-local
  references.
- A live, bounded catalog finder that delegates complete-snapshot discovery
  and document loading back to the host.
- Typed Markdown/man reference activation, safe external-URI delegation, and
  bounded back/forward history.
- Confirmed full-document search with active and inactive match highlighting.
- tldr quick-reference and source-document rendering through one layout model.
- Span-aware table columns and cell-local anchors that retain their exact
  rendered rows through independent wrapping, stacking and nested tables.
- Keyboard, mouse, scrollbar, and resizable-pane interaction.
- Width-aware visual text selection plus typed requests for plain-text and
  complete-node Text/Markdown clipboard content.
- A Crossterm lifecycle boundary that restores raw mode and the alternate
  screen after normal exit, setup failure, panic, or a handled POSIX
  termination signal before the signal's default action resumes.
- A static, less-like pager for process-owned textual query and catalog output;
  short results print directly. The CLI owns display/colour policy and bypasses
  the pager entirely for redirected automatic output and machine protocols.
- Public `App` and `DocumentView` layers for callers embedding the frontend in
  an existing Ratatui host.

Command-line parsing and document loading deliberately remain outside this
crate.

Entry title colors reflect source-neutral roles, not importance, confidence or
alias equivalence. Generic terms remain primary text. The body applies type
color only at validated name bindings; identical prose and list markers do not
inherit it. Source bold/italic/code styling and link underlines compose with
that color. Code-token accents do not overwrite semantic name roles.

`DocumentView` prepares a borrowed binding map during construction and stores
the resulting immutable styled lines. Resizing only reflows those lines;
search and selection overlay their own state without rewriting the base styles,
link targets or source coordinates.
Block indentation is a signed displacement from its parent's content origin.
The UI shares cell measurement, origin composition and marker collision rules
with the plain-text frontend through `mant-protocol`; only visible leaves are
bounded for padding. Source term roots keep their hard lines and zero-width
targets do not manufacture blank rows.

Width reduction is a view policy, not source layout. When indentation would
leave fewer than 16 content cells (half the width on narrow views), both first
and continuation origins are reduced by one common displacement, bounded at
the left edge. One/two-column views reserve their whole width for content;
a glyph that cannot fit uses a bounded display replacement while preserving
its search identity. This avoids one-character columns at ordinary widths.
Soft wrapping, link hit regions and search highlights share the same cell map.
See the shared [entry presentation contract](https://github.com/BryanHeBY/ManT/blob/main/docs/architecture/entry-presentation.md)
for label modes, source binding coordinates and style precedence.

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
exact logical document addresses, and an `ExternalUri` that has already passed
the shared HTTP(S)/mailto activation policy; rejected URI schemes remain
visible document text but never reach the callback. Callback failures return
to the UI as notices rather than giving the frontend hidden authority.
Typed email links use the IR-owned mailto serializer, so URI-sensitive mailbox
characters are encoded before the resulting `ExternalUri` crosses that same
activation boundary.

The upper-right document tab stack records successful loads in stable first-open
order and deduplicates logical addresses. Selecting an addressed tab emits the
ordinary `open_document` request and commits the active tab only after the host
returns content; the initial direct bundle remains locally restorable. Tabs
retain the last selected semantic node, use terminal-column-aware truncation,
and expose bounded overflow controls. The stack and navigation history are each
capped at 64 entries.

Completing an effective document-text drag emits its plain-text copy request
immediately. A successful callback produces a short-lived, non-modal
confirmation; selection extraction excludes presentation-only tldr panel
borders. Holding the pointer at either vertical viewport edge scrolls and
extends the active selection until release or the document limit. Like a text
editor, Shift-modified clicks and drags retain the original mouse-down anchor
and move the active endpoint, while keyboard and Edit-menu actions can copy the
retained selection again.

Selection copies visual cells, including visual line breaks and continuation
padding; it is not a source-format export. Use document export to retain source
logical lines independently of viewport width. Literal text is never rewritten
in IR by a resize, and its significant spaces remain selectable.

## Platform behavior

The static pager consumes already projected text rather than querying the
engine during resize or search. Search uses visible physical rows (excluding
ANSI control bytes), so regex anchors refer to the current wrapped rows.
No-result searches preserve a valid viewport and permit forward/backward search
without an index entry. Match overlays retain source foreground/font state at
the original glyphs; clearing search restores that base presentation. Native
pager regressions and the CLI's Unix PTY test verify search, resize and colors.
Declaration-group context belongs to the protocol projection, not a second
adjacency inference in the UI; full-document TUI layout ignores that metadata.

The frontend is portable across Linux, macOS, and Windows and does not inspect
the original document source. Callers on every supported platform can provide
normalized man, mdoc, or Markdown queries.

Install [`mant`](https://crates.io/crates/mant) for the complete executable.
This component crate is a library and does not install a second command.
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## License

Apache-2.0.
