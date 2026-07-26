:::tldr
# mantui

> Explore manuals and Markdown in a structured terminal UI.

- Open a local manual:

`mantui {{topic}}`

- Open a manual from a specific section:

`mantui {{topic}} --section {{section}}`

- Open a local Markdown document:

`mantui {{path/to/document.md}}`

- Open ManT's self-hosted interactive manual:

`mantui mantui.md`
:::

# mantui

## Name

`mantui` — explore complete local Unix manual pages and Markdown documents in
a structured terminal UI.

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
Manual section and renderer options apply only to manual topics.

`mantui` does not accept Markdown on standard input because a full-screen
reader owns the terminal input stream. Use `mant -` for piped content.

## Quick References

Manual pages can include a compatible local tldr page before the complete
manual. Markdown files can provide the same experience with a leading
`:::tldr` container. Both forms occupy reserved navigation path `0`, use the
same highlighted panel, participate in search, and keep the manual sections
one-based.

Embedded quick references use standard tldr placeholders such as
`{{path/to/file}}`. Placeholders, command options, numbers, and strings receive
code-aware terminal highlighting. Document-owned content does not display the
tldr-pages licence attribution used for community cache data.

## Markdown Documents

Markdown uses the same sidebar, search, links, lists, tables, code rendering,
and reading-position tracking as a manual. When the first heading is H1 it
names the document; remaining headings create the navigable hierarchy.
Non-empty introductory content outside that section tree appears as an
`OVERVIEW` entry.

The semantic subset includes paragraphs, strong and emphasized text, inline
code, hard breaks, standard and page-local links, fenced and indented code
blocks, ordered and unordered nested lists, thematic breaks, and GFM tables.
Unsupported browser-oriented constructs remain visible with a diagnostic
instead of disappearing.

Two explicit ManT extensions add manual-like behavior:

- A `:::tldr` container at the first non-empty line becomes the independent
  zero-position quick reference. Its contents use the tldr-pages command,
  description, example, and `{{placeholder}}` conventions.
- A complete bullet list written as ``- `--flag`: description`` becomes
  semantic options. These entries expand beneath their section and can be
  selected like options parsed from roff.

The `mant` manual documents the exact supported syntax, preservation behavior,
and extension grammar.

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
- `MANPATH`: Supply manual-hierarchy roots containing directories such as
  `man1/` to the companion's host `man -w` lookup.
- `MANSECT`: Set the host manual implementation's default section order when
  `--section` is absent.
- `MANT_TLDR_DIR`: Select an explicit tldr checkout through the companion engine.
- `MANT_DEBUG`: Include JavaScript stack diagnostics in unexpected TUI failures.

Standard cache and locale variables used by `mant` also apply because the
companion process inherits the environment.

## Exit Status

`0` indicates a normal exit or successful help request, `2` indicates invalid
command-line usage, and `1` indicates an operational or TUI failure.

## See Also

`mant` provides outlines, excerpts, semantic option explanations, search,
Markdown, text, JSON, generated schemas, and MCP stdio for agents and scripts.
