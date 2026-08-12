# mant-ui

`mant-ui` is the Ratatui frontend component used by the `mant` executable. It
renders ManT's normalized `mant-ast::QueryBundle` and owns interactive
navigation, search, scrolling, links, menus, mouse input, and terminal
presentation.

## What this crate provides

- A hierarchy-aware, collapsible sidebar for sections and every semantic entry
  role: options, commands, variables, and environment variables.
- Settled-scroll navigation following and selectable page-local references.
- A live catalog finder that delegates document loading back to the host.
- Confirmed full-document search with active and inactive match highlighting.
- tldr quick-reference and source-document rendering through one layout model.
- Keyboard, mouse, scrollbar, and resizable-pane interaction.
- A Crossterm lifecycle boundary that restores raw mode and the alternate
  screen after normal exit, setup failure, or panic.
- Public `App` and `DocumentView` layers for callers embedding the frontend in
  an existing Ratatui host.

Command-line parsing and document loading deliberately remain outside this
crate.

## Basic use

The convenience boundary owns the terminal event loop:

```rust,no_run
let query = mant_core::query_markdown_text(
    "# Demo\n\n## Overview\n\nHello from ManT.\n",
    Some("demo.md".to_owned()),
)?;

mant_ui::run(&query)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`run` requires an interactive terminal. Callers that already own a Ratatui
event loop can construct `mant_ui::App`, route input through its handlers, and
invoke `App::draw` from their frame callback instead.

## Platform behavior

The frontend is portable across Linux, macOS, and Windows and does not inspect
the original document source. Callers on every supported platform can provide
normalized man, mdoc, or Markdown queries.

Install [`mant`](https://crates.io/crates/mant) for the complete executable.
This component crate is a library and does not install a second command.

## License

Apache-2.0.
