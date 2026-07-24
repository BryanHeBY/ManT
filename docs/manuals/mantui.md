# mantui

Explore local Unix manual pages and Markdown in a structured terminal UI.

## TLDR Quick Reference

Open a local manual:

```sh
mantui git
mantui printf --section 3
```

Open a Markdown document:

```sh
mantui README.md
mantui mant.md
```

## Synopsis

```text
mantui <TOPIC|MARKDOWN> [--section SECTION] [--force-libmandoc] [--force-groff]
mantui -h
```

## Description

`mantui` is ManT's interactive reader. Its resizable sidebar follows the
document hierarchy and reading position, while the content pane preserves
structured prose, definitions, code, lists, tables, links, and layout.

Local manual topics and Markdown paths are queried through the companion
`mant` executable. `mantui` finds that executable through `MANT_PATH` first
and then `PATH`.

## Input

Values ending in `.md` or `.markdown`, and other path-like values, are read as
local Markdown files. Other values are resolved as local manual topics.

An exact Markdown heading named `TLDR`, `TLDR Quick Reference`, or
`Quick Reference` remains part of the document but receives the same distinct
navigation and content styling as ManT's external tldr preface.

## Options

- `-h`, `--help`: Show command help and exit.
- `-s SECTION`, `--section SECTION`: Select a manual section such as `1` or `3p`.
- `--force-libmandoc`: Require direct libmandoc output and print parser diagnostics.
- `--force-groff`: Use the opt-in groff HTML compatibility path.

The `--` separator treats all remaining arguments as the manual topic.

Manual section and renderer options are rejected for Markdown input.

## Navigation

Use the sidebar, mouse wheel, or keyboard to move through the document.
Selecting a sidebar item places its heading at the top of the content pane.
After scrolling stops, the sidebar follows the first visible document node.

- `j`, `Down`: Select the next visible node.
- `k`, `Up`: Select the previous visible node.
- `h`, `Left`: Collapse the current branch or select its parent.
- `l`, `Right`: Expand the current branch or select its first child.
- `d`, `PageDown`: Scroll the content down.
- `u`, `PageUp`: Scroll the content up.

## Search

- `Ctrl+F`, `/`: Open the bottom search field.
- `Enter`: Confirm a query or select the next match.
- `n`: Select the next confirmed match.
- `Shift+N`: Select the previous confirmed match.
- `Escape`: Close search and remove match highlighting.

Search runs only after confirmation. All matches remain visible, while the
active match uses a stronger highlight and is placed at the top of the content
viewport.

## Interface

- `F10`: Open the classic menu bar.
- `?`: Show keyboard shortcuts.
- `q`: Quit.

The sidebar can be hidden from the View menu and resized by dragging its
boundary.

## Environment

- `MANT_PATH`: Absolute or relative path to the companion `mant` executable.

## See Also

`mant` provides outlines, excerpts, semantic option explanations, search,
Markdown, text, JSON, generated schemas, and MCP stdio for agents and scripts.
