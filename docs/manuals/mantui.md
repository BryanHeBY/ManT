# mantui

Explore complete local Unix manual pages in a structured terminal UI. The
same reader also opens local Markdown documents.

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

`mantui` is ManT's interactive reader for people. Its resizable sidebar turns
manual sections and semantic options into navigation, follows the settled
reading position, and keeps page-local references directly usable. The
content pane preserves structured prose, definitions, code, lists, tables,
links, and layout.

Local manual topics and Markdown paths are queried through the companion
`mant` executable. `mantui` finds that executable through `MANT_PATH` first
and then `PATH`.

## Input

Ordinary values are resolved as local manual topics. Values ending in `.md` or
`.markdown`, and other path-like values, are read as local Markdown files.

Manual pages can include an external tldr quick reference before the complete
source document when compatible local tldr data is available.

## Markdown Documents

Markdown uses the same sidebar, search, links, lists, tables, code rendering,
and reading-position tracking as a manual. Content before the first heading
appears as an `OVERVIEW` entry.

An exact heading named `TLDR`, `TLDR Quick Reference`, or `Quick Reference`
remains part of the document but receives the same distinct navigation and
content styling as ManT's external tldr preface. Option lists written as
``- `--flag`: description`` become expandable semantic entries in the
sidebar.

Unsupported Markdown syntax remains visible with a diagnostic rather than
being silently discarded. Standard-input Markdown belongs to the
non-interactive `mant -` workflow; `mantui` accepts file paths.

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
