:::tldr
# mant

> Turn local Unix manual pages and Markdown into structured documents.

- Read a complete manual interactively:

`mant {{name}}`

- Inspect a manual outline:

`mant {{name}} --outline`

- Retrieve one section from that outline:

`mant {{name}} --node {{path}}`

- Explain one option directly:

`mant {{name}} --explain={{option}}`

- Read a local Markdown file:

`mant {{path/to/document.md}}`

- Read Markdown from standard input:

`cat {{path/to/document.md}} | mant -`
:::

# mant

## Name

`mant` — read and query structured local Unix manual pages and Markdown.

## Synopsis

```text
mant <NAME|MARKDOWN|-> [OPTIONS]
mant --request-json [--format FORMAT] [--compact]
mant --schema CONTRACT [--compact]
mant --update-tldr [--compact]
mant --protocol-version [--compact]
mant --mcp
```

## Description

`mant` is ManT's native interactive reader, structured command-line interface,
and MCP server. It parses local man and mdoc sources through bundled libmandoc,
then exposes their hierarchy, semantic options, references, and visible text
through one normalized document model.

Local Markdown enters the same model, so terminal navigation, outlines,
excerpts, search, Markdown/text/JSON output, and MCP tools behave consistently
across both sources. A full query opens the interactive reader when stdin and
stdout are terminals; redirection falls back to clean Markdown. `--ui` and
`--format` make either behavior explicit.

## Input

Input is resolved before parsing. An ordinary value first selects a registered
Markdown document from `${XDG_DATA_HOME:-$HOME/.local/share}/mant`, then
from each `$XDG_DATA_DIRS/mant` directory, and finally from the native
manual index. Values ending in `.md` or `.markdown`, other path-like values, and
the exact value `-` select Markdown directly instead.

The filename supplies a registered document name: `mant.md` is queried as
`mant mant`. Registration roots are scanned recursively, including file and
directory symbolic links. Directories organize documents but do not form
namespaces: `team/handbook.md` is still queried as `mant handbook`.

For duplicate names, user data precedes system data, shallower paths precede
deeper paths, relative path order breaks remaining ties, and `.md` precedes
`.markdown` in the same directory. Broken links and unreadable paths are
ignored; canonical directory identities prevent symbolic-link cycles. An
explicit `--manual` or `--section` request bypasses registered Markdown and
selects a native manual page.

### Manual Pages

Manual page names are located through ManT's native manual index. ManT reads
raw, gzip, and zstd roff sources and lowers man or mdoc semantics through its
bundled libmandoc parser. Neither a system `man` nor a system `mandoc`
executable is required for ordinary use.

- `--section SECTION`: Select a manual section such as `1` or `3p`.
- `--manual`: Require a native manual instead of registered Markdown with the
  same name.

Recoverable parser findings remain structured in JSON output. ManT does not
invoke a host renderer or maintain an alternate HTML parsing path.

### Local Roff Trees

Use `MANT_MANPATH` or `MANPATH` to make project-local man or mdoc sources
visible without copying them into a system directory. Each entry must be the
root of a hierarchy containing section directories such as `man1/` or
`man3/`; it must not name the section directory or source file itself.

For example, expose `widget.1` as document name `widget`:

```sh
mkdir -p ./project-man/man1
cp ./widget.1 ./project-man/man1/widget.1
MANT_MANPATH="$PWD/project-man" mant widget --section 1
```

`MANT_MANPATH` is a complete ManT-specific override. `MANPATH` also replaces
the derived defaults unless it contains an empty component; an empty component
inserts user/XDG, PATH-derived, and conventional system roots at that point.
This preserves familiar colon-list behavior without invoking `man`.

Do not pass `./widget.1` as the input operand: path-like operands are reserved
for Markdown documents. Register local roff in a manual hierarchy and query
it by name instead.

## Markdown Documents

Local files and standard input enter the same document, outline, excerpt,
search, and output pipeline as manual pages. The supported subset is
deliberately structural rather than a complete browser-oriented Markdown
implementation.

### Supported Structure

- When the first heading is H1, it supplies the document title and does not
  consume an outline number.
- All remaining H1 through H6 headings form the recursive, one-based section
  tree; H2 is the conventional top level after a document title. Repeated
  headings receive unique document-local IDs.
- Prose before the first heading, and prose belonging to the document title,
  becomes the selectable `root` overview when it is non-empty.
- Paragraphs, thematic breaks, fenced and indented code blocks, ordered and
  unordered lists, nested lists, and GFM tables are structured nodes.
- A fenced code block retains the first language token from its info string.
  Table column alignment and ordered-list start values are also retained.

### Supported Inline Syntax

Plain text, strong and emphasized text, inline code, explicit hard line
breaks, standard links, email links, and document-local fragment links remain
semantic inline nodes. Soft source line breaks become spaces, while hard
breaks remain visible. A resolved fragment link can be followed directly in
the reader; external and email destinations remain available to other
consumers.

### ManT Extensions

A complete bullet list becomes semantic command-line options when every item
begins with one or more code spans containing option names, followed by `:`,
`—`, or `–` and a description:

```markdown
- `-h`, `--help`: Show help.
- `--color WHEN`: Select a colour mode.
```

Aliases may be separated by commas, slashes, or vertical bars. Recognized
entries receive stable IDs and aliases, appear beneath their owning section
in `--outline`, and are selectable through `--node` and `--explain`. A mixed
ordinary/option list remains an ordinary list rather than being guessed.

An optional leading `:::tldr` container is parsed independently with the
tldr-pages dialect:

```markdown
:::tldr
# tool

> One-line command description.
> More information: https://example.test/tool.

- Run the command for a file:

`tool {{path/to/file}}`
:::

# Tool
```

The opening marker must be the first non-empty construct and the closing line
must contain only `:::`. The embedded page uses a command H1, quoted
description lines, optional `More information:`, example descriptions, code
commands, and standard `{{placeholder}}` or `{{[-s|--long]}}` placeholders.
It receives path `0` and alias `tldr`, uses the same renderer and search model
as cached tldr-pages data, and records document-owned provenance without
claiming tldr-pages licence attribution. Ordinary headings named `TLDR` have
no special meaning.

### Preserved Unsupported Syntax

Block quotes, task lists, images, raw HTML, strikethrough, footnotes, native
Markdown definition lists, metadata blocks, math, superscript, subscript, and
wiki links are outside the semantic subset. Their source remains visible as
an unsupported node or inline fragment, and JSON results include a source
diagnostic. ManT does not silently discard them or pretend to render browser
features it does not implement.

Source spans keep original Markdown line and column positions. Extracting an
embedded tldr preface does not shift those coordinates.

## Interactive Reader

With a complete document name or Markdown path and a terminal on stdin and stdout,
`mant` opens its Ratatui reader. `--ui` requires this mode explicitly. A
projection option or `--format` selects deterministic output instead.

The resizable sidebar mirrors the section hierarchy and semantic options.
Selecting an item puts its heading at the top of the content pane. After
content scrolling settles, the sidebar follows the first visible section.
Page-local references can be followed directly.

### Navigation

- `j`, `Down`: Select the next visible node.
- `k`, `Up`: Select the previous visible node.
- `h`, `Left`: Collapse the current branch or select its parent.
- `l`, `Right`: Expand the current branch or select its first child.
- `d`, `PageDown`: Scroll the content down.
- `u`, `PageUp`: Scroll the content up.

Mouse input selects and folds navigation entries, follows page-local links,
scrolls either pane, drags scrollbars, and resizes the sidebar boundary.

### Page Search

- `Ctrl+F`, `/`: Open the bottom search field.
- `Enter`: Confirm a query or select the next match.
- `n`: Select the next confirmed match.
- `Shift+N`: Select the previous confirmed match.
- `Escape`: Close search and remove match highlighting.

Search runs only after confirmation. Every match stays highlighted while the
active match uses a stronger background and moves into view.

### Interface

- `F10`: Open the menu bar.
- `?`: Show keyboard shortcuts.
- `q`: Quit.

The View menu can hide the sidebar. Terminal setup is restored on normal exit,
errors, and Rust panics.

## Document Selection

- `--outline[=DETAIL]`: Print the addressable tree; `options` is the default and `sections` is the compact form.
- `--node NODE`: Return a node by path or ID; repeat the option to select several nodes.
- `--explain ENTRY`: Return exactly one semantic option, command, or environment entry.

Path `0` and ID alias `tldr` are reserved for either an external tldr page or a
Markdown document's explicit `:::tldr` preface. Remaining headings use
one-based paths such as `2.3`, and semantic entries use paths such as `2.3/o4`.

## Search

- `--search PATTERN`: Search visible text and report reusable nodes plus Markdown coordinates.
- `--grep PATTERN`: Alias for `--search`.
- `--regex`: Interpret the pattern as a regular expression.
- `--case POLICY`: Use `insensitive`, `sensitive`, or `smart` case handling.
- `--word`: Require Unicode-aware word boundaries.
- `--scope SCOPE`: Search `visible` text or generated `markdown`.
- `--context LINES`: Include surrounding Markdown lines.
- `--limit COUNT`: Limit returned matches.
- `--offset COUNT`: Skip matches for deterministic pagination.

Search defaults to a case-insensitive literal over visible text, returns at
most 100 matches, and includes no context lines. `smart` case becomes
case-sensitive when the pattern contains uppercase text. Every result still
uses generated Markdown as its coordinate space, so line and column ranges
can be passed between text, JSON, and agent workflows.

Use the `=` form when a value begins with a hyphen:

```sh
mant tar --search=--acls
mant tar --explain=--exclude
```

## Output

- `--format FORMAT`: Select `markdown`, `text`, `man`, or `json`.
- `--compact`: Omit JSON indentation.
- `--preserve-anchors`: Retain addressable HTML anchors in Markdown output.

Clean Markdown output omits internal HTML anchors by default. The `man` format
is plain manual content without an external tldr preface. Explicit output for
full documents and excerpts defaults to Markdown; outlines and command-line
search default to text. JSON must be selected explicitly, and `--compact` is
valid only with JSON query output.

## Integration

- `--request-json`: Read one closed `mant.request/v4` object from standard input.
- `--schema CONTRACT`: Print a generated JSON Schema for `request`, `query`, `outline`, `excerpt`, `search`, or `all`.
- `--protocol-version`: Print the exact native protocol versions.
- `--mcp`: Serve read-only ManT tools over silent MCP stdio. Lowering
  diagnostics are omitted; inspect them with ordinary CLI JSON output.

### Protocol Discovery

The current protocol descriptor is:

```json
{
  "protocol": "mant.cli/v4",
  "nativeApiVersion": "4",
  "requestSchema": "mant.request/v4",
  "querySchema": "mant.query/v4",
  "documentSchema": "mant.document/v4",
  "outlineSchema": "mant.outline/v4",
  "excerptSchema": "mant.excerpt/v4",
  "searchSchema": "mant.search/v4"
}
```

These contracts use one coherent unpublished `v4` suite for the first public
protocol release. Future revisions may advance individual contracts only when
their wire shapes change, so consumers must still compare every exact schema
identifier. Generated schemas use JSON Schema Draft 2020-12 and remain the
authoritative field-level definition.
The repository's `docs/protocol.md` supplies the complete field reference,
examples, compatibility policy, coordinate rules, and MCP tool contracts.

Standard output is reserved for the requested result. Concise diagnostics use
standard error. `--request-json` accepts the same input and projection model
used by external process integrations. MCP exposes `mant_documents_list`,
`mant_document_outline`, `mant_document_get`, `mant_document_explain`, and
`mant_document_search`. Document tools accept a name and optional manual
section, not an arbitrary local path; register Markdown before exposing it to
an agent. `mant_documents_list` merges registered Markdown with the native
manual index and supports a case-insensitive `query`, `markdown` or `manual`
`kind`, exact `section`, and bounded `limit`/`offset` pagination.

## Data

- `--update-tldr`: Update through an installed tldr client when available, otherwise through ManT's private cache.

Normal queries prefer compatible installed-client data. If no client is
installed, ManT reads its private cache. Manual content remains usable when no
tldr page is available.

## Environment

- `MANT_MANPATH`: Completely replace ManT's manual roots with a colon-separated
  list. Each root contains section directories such as `man1/`, not an
  individual roff file.
- `MANPATH`: Override the derived manual roots. Empty components insert the
  user/XDG, PATH-derived, and conventional system roots.
- `MANT_TLDR_DIR`: Use one explicit tldr checkout for reads and updates.
- `XDG_CACHE_HOME`: Relocate cache discovery and ManT's Linux fallback cache.
- `XDG_DATA_HOME`: Relocate the user document directory from the default
  `$HOME/.local/share/mant`.
- `XDG_DATA_DIRS`: Add system data roots considered during
  registered-document and tldr discovery. Registered Markdown is read
  recursively from each `mant` subdirectory.
- `LC_ALL`, `LC_MESSAGES`, `LANGUAGE`, `LANG`: Select localized manual sources
  and translated tldr pages before English fallback.
- `HOME`: Supply conventional document, manual, and cache locations when their
  XDG overrides are absent.

## General

- `--ui`: Require the interactive reader instead of automatic terminal detection.
- `-h`, `--help`: Show command help and exit.
- `-V`, `--version`: Show the installed ManT version and exit.

## Exit Status

`0` indicates success, `2` indicates invalid input or usage, and `1` indicates
an operational failure.

## See Also

`docs/protocol.md` documents the JSON and MCP contracts used by external
integrations.
