<!-- mant:tldr:start -->
# mant

> Turn local manual pages and cross-platform Markdown into structured documents.

- Read a complete manual interactively:

`mant {{name}}`

- Inspect a manual outline:

`mant {{name}} --outline [=sections]`

- Retrieve one section from that outline:

`mant {{name}} --node {{path}}`

- Explain one option directly:

`mant {{name}} --explain={{option}}`

- Read a local Markdown file:

`mant {{path/to/document.md}}`

- Read Markdown from standard input:

`cat {{path/to/document.md}} | mant -`

- Update configured document sources:

`mant --update-docs`
<!-- mant:tldr:end -->

# mant

## Name

`mant` — read and query structured manual pages and cross-platform Markdown.

## Synopsis

```text
mant <NAME|MARKDOWN|-> [OPTIONS]
mant --request-json [--format FORMAT] [--compact]
mant --schema CONTRACT [--compact]
mant --update-docs [--compact]
mant --update-tldr [--compact]
mant --protocol-version [--compact]
mant --mcp
```

## Description

`mant` is ManT's native interactive reader, structured command-line interface,
and MCP server. Linux with glibc, macOS, and Windows parse local man and mdoc
sources through bundled libmandoc. Every supported platform exposes hierarchy,
semantic entries, references, and visible text through one normalized document
model.

Local Markdown enters the same model, so terminal navigation, outlines,
excerpts, search, Markdown/text/JSON output, and MCP tools behave consistently
across both sources. A full query opens the interactive reader when stdin and
stdout are terminals; redirection falls back to clean Markdown. `--ui` and
`--format` make either behavior explicit.

## Input

Input is resolved before parsing. An ordinary value first checks the user's
flat `documents` directory, then configured installed sources by descending
priority and source name in ascending bytewise order, and finally the native
manual index. Linux uses
`${XDG_DATA_HOME:-$HOME/.local/share}/mant`, macOS uses
`~/Library/Application Support/ManT`, and Windows uses `%APPDATA%\ManT` as its
data root. Values ending in `.md` or `.markdown`, other path-like values, and
the exact value `-` select Markdown directly instead.

The filename supplies a registered document name: `mant.md` is queried as
`mant mant`. Only regular `.md` and `.markdown` files immediately inside a
registered directory are visible. Nested directories and symbolic links are
ignored. Root documents always win; source priority and name resolve remaining
duplicates in the order above; `.md` wins over `.markdown` within one
directory. `--source NAME` selects exactly one configured Git or archive
source. `--manual` or `--section` selects a native manual and cannot be combined
with `--source`.

Windows document packages should retain executable suffixes in canonical
filenames, such as `cargo.exe.md`. An extensionless query such as `mant cargo`
first tries the exact name, then appends extensions from `PATHEXT` in order, so
it can resolve `cargo.exe`; an explicit `mant cargo.exe` remains exact. This is
a Windows platform rule independent of the calling shell and PowerShell
version. Script suffixes such as `.ps1` participate only when present in
`PATHEXT`; an unset or empty value uses `.COM`, `.EXE`, `.BAT`, and `.CMD`.
Other platforms never elide these suffixes.

### Manual Pages

On Linux with glibc, macOS, and Windows, manual page names are located through
ManT's native manual index. ManT reads raw, gzip, and zstd roff sources and
resolves redirect-only `.so` alias chains within the indexed manual root. All
file and decompression I/O for manual sources remains in ManT; bundled
libmandoc receives only the final plain roff bytes. Neither a system `man` nor
a system `mandoc` executable is required for ordinary use.

Windows uses `%USERPROFILE%\.local\share\man` as its conventional user root
and accepts additional roots through `MANPATH` or `MANT_MANPATH`.

- `--section SECTION`: Select a manual section such as `1` or `3p`.
- `--manual`: Require a native manual instead of registered Markdown with the
  same name.
- `--source SOURCE`: Require one configured installed Markdown source.

Recoverable parser findings remain structured in JSON output. ManT does not
invoke a host renderer or maintain an alternate HTML parsing path.

### Local Roff Trees

Use `MANT_MANPATH` or `MANPATH` to make project-local man or mdoc sources
visible without copying them into a system directory. Each entry names a root;
pages can live directly below it as `widget.1`, or in section directories such
as `man1/widget.1`. The entry must not name an individual source file.

For example, expose `widget.1` as document name `widget`:

```sh
mkdir -p ./project-man/man1
cp ./widget.1 ./project-man/man1/widget.1
MANT_MANPATH="$PWD/project-man" mant widget --section 1
```

The equivalent PowerShell setup is:

```powershell
New-Item .\project-man\man1 -ItemType Directory -Force | Out-Null
Copy-Item .\widget.1 .\project-man\man1\widget.1
$env:MANT_MANPATH = (Resolve-Path .\project-man).Path
mant widget --section 1
```

`MANT_MANPATH` is a complete ManT-specific override. `MANPATH` also replaces
the derived defaults unless it contains an empty component; an empty component
inserts user/XDG, PATH-derived, and conventional system roots at that point.
This preserves familiar path-list behavior without invoking `man`. Unix uses
colon-separated entries; Windows uses semicolon-separated entries. Its
conventional fallback contains only `%USERPROFILE%\.local\share\man`; other
locations remain explicit.

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

A complete bullet list becomes semantic Unix command-line options when every
item begins with one or more code spans containing dash-option names, followed
by `:`, `—`, or `–` and a description:

```markdown
- `-h`, `--help`: Show help.
- `--color WHEN`: Select a colour mode.
```

This legacy shorthand is case-sensitive. Aliases may be separated by commas,
the historical `-h/--help` notation, or vertical bars.

Role-aware entries, including Windows switches, commands, and environment
variables, use an explicit invisible directive before one complete bullet
list. Both the role and matching policy are required:

```markdown
<!-- mant:entries role=option case=insensitive -->
- `/query`: Query scheduled tasks.
- `/?`: Display help.
- `/S COMPUTER`: Select a remote computer.
- `/server:NAME`: Select a server.
- `/reg:32`, `/reg:64`: Select registry views.
- `type= TYPE`: Select a service type.
- `//B`: Select Windows Script Host batch mode.
- `+r`: Set an attribute.
- `/+N`: Select a character offset.
- `/driver.exclude`: Select Driver Verifier exclusions.

<!-- mant:entries role=option case=insensitive attached=fixed -->
- `/F`: Run an extended scan.
- `/F:Y`: Run an extended scan and clean detected malware.
- `/server:<NAME>`: Select a server while keeping an explicit placeholder.
- `perf=default`: Select a fixed named policy.

<!-- mant:entries role=command case=insensitive -->
- `query`: Read keys and values.
- `winget install`: Install a package.

<!-- mant:entries role=environment-variable case=insensitive -->
- `PATH`, `$env:PATH`: Control executable discovery.
- `$LASTEXITCODE`: Hold the last native process exit code.
```

`role` is `option`, `command`, or `environment-variable`; `case` is
`sensitive` or `insensitive`. Option declarations may additionally use
`attached=fixed`. It retains unbracketed values attached with `:` or `=` as
part of the semantic name, so `/F:Y` and `perf=default` remain distinct from
`/F` and `perf=`. An angle-bracketed value such as `<NAME>` remains an explicit
placeholder. Omitting the field preserves the legacy inference in which an
uppercase attached value is a placeholder. `attached=infer` states that
default explicitly. The directive must be the only construct on its line and
targets a bullet list beginning on the next non-empty line. Blank lines are
allowed, but a heading, paragraph, or other intervening construct invalidates
the declaration.

Every item in a declared list must begin with one or more inline-code terms,
optionally separated by commas or `|`, and then contain `:`, `—`, or `–` in the
same leading paragraph. The delimiter is required even when further
description blocks follow. For example:

```markdown
<!-- mant:entries role=option case=insensitive -->
- `/query`, `/Q`: Query the current state.
```

This is intentionally not a semantic entry because the leading paragraph has
no description delimiter:

```markdown
<!-- mant:entries role=option case=insensitive -->
- `/query`, `/Q`
  Query the current state.
```

Value placeholders remain visible while normalized selectors omit them.
Whitespace placeholders such as `/S COMPUTER` become `/S`. A colon suffix in
uppercase placeholder form, optionally enclosed in angle brackets, is also
omitted: `/server:NAME` becomes `/server`, as does `/server:<NAME>`.
Lowercase colon suffixes are fixed values, so `/server:name` remains the full
selector. Numeric and lowercase alphabetic values such as `/reg:32` and
`/mode:auto` likewise remain part of the selector.

Declared option lists also accept conservative Windows-native token families.
An ASCII identifier followed by `=` retains the equals sign while omitting an
uppercase or angle-bracket placeholder, so `type= TYPE` and `board=N` become
`type=` and `board=`. Windows Script Host `//` names preserve both slashes and
apply the same colon-placeholder rule. Safe leading-plus, slash-plus, and
dotted-slash tokens such as `+r`, `/+N`, and `/driver.exclude` remain complete
selectors. Arbitrary prose, paths, empty dotted segments, and lowercase
equals-value placeholders are rejected.

Case policy belongs only to the declared list. It does not change section
paths or IDs. Use `sensitive` when `-p` and `-P` differ, and `insensitive` for
Windows `/S` versus `/s`, PowerShell command and parameter names, or
environment spellings such as `PATH`, `$env:PATH`, and `$ENV:PATH`. Outlines
always retain the canonical spelling written by the author.

Unknown fields, missing policies, and malformed declared lists produce a
source-located recoverable diagnostic without dropping or reordering content.

Recognized entries receive role-specific stable IDs and aliases, appear
beneath their owning section in `--outline`, and are selectable through
`--node` and `--explain`. A mixed ordinary/option list remains an ordinary list
rather than being guessed. When an alias occurs more than once, selection
fails with candidate paths and IDs; use one of those stable qualifiers.

Semantic tables are not currently inferred or declared. Convert an interface
table to a declared bullet list when its rows must appear in outlines and work
with `--explain`; ordinary tables remain ordinary document content.

An optional leading tldr preface is parsed independently with the tldr-pages
dialect. Invisible CommonMark HTML comments delimit it, so GitHub renders the
contents normally without exposing extension syntax:

```markdown
<!-- mant:tldr:start -->
# tool

> One-line command description.
> More information: https://example.test/tool.

- Run the command for a file:

`tool {{path/to/file}}`
<!-- mant:tldr:end -->

# Tool
```

The opening line must contain only `<!-- mant:tldr:start -->` and be the first
non-empty construct. The closing line must contain only
`<!-- mant:tldr:end -->`. The embedded page uses a command H1, quoted
description lines, optional `More information:`, example descriptions, code
commands, and standard `{{placeholder}}` or `{{[-s|--long]}}` placeholders.
Adjacent source lines in one quoted or example-description paragraph are joined
according to CommonMark; source formatting therefore does not force terminal
line breaks.
It receives path `0` and alias `tldr`, uses the same renderer and search model
as cached tldr-pages data, and records document-owned provenance without
claiming tldr-pages licence attribution. Ordinary headings named `TLDR` have
no special meaning.

### Preserved Unsupported Syntax

Block quotes, task lists, images, raw HTML, strikethrough, footnotes, native
Markdown definition lists, metadata blocks, math, superscript, subscript, and
wiki links are outside the semantic subset. The two exact leading tldr
boundary comments are consumed before ordinary Markdown lowering; all other
raw HTML follows the unsupported-syntax rule. Its source remains visible as an
unsupported node or inline fragment, and JSON results include a source
diagnostic. ManT does not silently discard it or pretend to render browser
features it does not implement.

Source spans keep original Markdown line and column positions. Extracting an
embedded tldr preface does not shift those coordinates.

## Interactive Reader

With a complete document name or Markdown path and a terminal on stdin and stdout,
`mant` opens its Ratatui reader. `--ui` requires this mode explicitly. A
projection option or `--format` selects deterministic output instead.

The resizable sidebar mirrors the section hierarchy and groups every semantic
entry role: options, commands, and environment variables.
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

- `--outline [DETAIL]`: Print the addressable tree; `entries` is the default and
  `sections` is the compact form. The CLI accepts historical `options` as an
  alias for `entries`.
- `--node NODE`: Return a node by path or ID; repeat the option to select
  several nodes.
- `--explain ENTRY`: Return exactly one semantic option, command, or
  environment entry.

Path `0` and ID alias `tldr` are reserved for either an external tldr page or a
Markdown document's explicitly marked tldr preface. Remaining headings use
one-based paths such as `2.3`, and semantic entries use paths such as `2.3/o4`.

`--node` first recognizes the reserved tldr and document-root selectors, then
resolves exact paths or IDs across sections and entries, and finally resolves
entry aliases. `--explain` is entry-only: it resolves an exact entry path or
ID first, then entry aliases using their declared case policy. Duplicate
aliases return deterministic candidate paths and IDs. Only when no entry
matches does an exact section, root, or tldr selector produce the instruction
to use `--node`; consequently a command alias may have the same spelling as a
section ID without being shadowed.

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
- `--preserve-anchors`: Retain addressable HTML anchors in full-document or excerpt Markdown output.

Clean Markdown output omits internal HTML anchors by default. The `man` format
applies to a complete native roff manual and emits plain manual content without
an external tldr preface; it rejects a complete Markdown document. Use `text`
for plain projections. Explicit output for full documents and excerpts defaults
to Markdown; outlines and command-line search default to text. JSON must be
selected explicitly for document queries. `--compact` removes indentation from
JSON queries, schemas, protocol descriptions, and update reports.

## Integration

- `--request-json`: Read one closed `mant.request/v5` object from standard input.
- `--schema CONTRACT`: Print a generated JSON Schema for `request`, `query`, `outline`, `excerpt`, `search`, or `all`.
- `--protocol-version`: Print the exact native protocol versions.
- `--mcp`: Serve read-only ManT tools over silent MCP stdio. Lowering
  diagnostics are omitted; an incomplete entry outline retains only
  `entriesComplete: false`. Inspect details with ordinary CLI or request JSON.

### Protocol Discovery

The current protocol descriptor is:

```json
{
  "protocol": "mant.cli/v5",
  "nativeApiVersion": "5",
  "requestSchema": "mant.request/v5",
  "querySchema": "mant.query/v5",
  "documentSchema": "mant.document/v5",
  "outlineSchema": "mant.outline/v5",
  "excerptSchema": "mant.excerpt/v5",
  "searchSchema": "mant.search/v5"
}
```

Request and response contracts advanced to v5 for explicit source selection,
the first-class explain view, and role- and case-aware semantic entries. The
independent `mant.markdown/v1` search-coordinate contract remains unchanged.
Future revisions may advance individual contracts only when their wire shapes
change, so consumers must compare every exact schema identifier. Generated
schemas use JSON Schema Draft 2020-12 and remain the authoritative field-level
definition.
The online [protocol reference](https://github.com/BryanHeBY/ManT/blob/main/docs/protocol.md)
supplies the complete field reference, examples, compatibility policy,
coordinate rules, and MCP tool contracts.

Standard output is reserved for the requested result. Concise diagnostics use
standard error. `--request-json` accepts the same input and projection model
used by external process integrations. MCP exposes `mant_documents_list`,
`mant_document_outline`, `mant_document_get`, `mant_document_explain`, and
`mant_document_search`. Document tools accept a name and optional source or
manual section, not an arbitrary local path. `mant_documents_list` merges
local Markdown candidates with the native manual index and supports `query`,
`kind`, exact `source` or `section`, and bounded pagination. MCP reads current
local files only; it has no update tool and no cross-call snapshot guarantee.

## Data

- `--update-docs`: Update Git or direct archive sources declared in `sources.toml` and print a complete JSON report.
- `--update-tldr`: Update through an installed tldr client when available, otherwise through ManT's private cache.

Git-backed document sources require a `git` executable on `PATH`; direct
archives use built-in download and extraction support. The private tldr cache
fallback also requires Git when no installed client performs the update.

Normal queries prefer compatible installed-client data and always retain
ManT's private cache as the final fallback. Manual content remains usable when
no tldr page is available.

## Storage

ManT keeps durable documents and source metadata separate from disposable
caches. On Linux they live below
`${XDG_DATA_HOME:-$HOME/.local/share}/mant`; the private tldr checkout lives
below `${XDG_CACHE_HOME:-$HOME/.cache}/mant/tldr-pages`.

On macOS, documents live below `~/Library/Application Support/ManT` and the
private tldr checkout lives below `~/Library/Caches/ManT/tldr-pages`.

On Windows, documents live below `%APPDATA%\ManT`. ManT's private
tldr checkout lives below `%LOCALAPPDATA%\ManT\cache\tldr-pages`. The native
manual fallback root is `%USERPROFILE%\.local\share\man`.

`sources.toml` lives at the data root. Personal documents and installed source
directories remain below `documents/`; see the online
[document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md)
for the schema and update lifecycle.

## Environment

- `MANT_MANPATH`: Completely replace ManT's manual roots. Lists use colons on
  Unix and semicolons on Windows. A root may contain flat roff files or section
  directories such as `man1/`, but is never an individual roff file.
- `MANPATH`: Override the derived manual roots. Empty components insert the
  user/XDG, PATH-derived, and conventional system roots.
- `MANT_TLDR_DIR`: Use one explicit tldr checkout for reads and updates.
- `XDG_CACHE_HOME`: Relocate cache discovery and ManT's Linux fallback cache.
- `XDG_DATA_HOME`: Relocate the user document directory from the default
  `$HOME/.local/share/mant/documents` on Linux.
- `XDG_DATA_DIRS`: Add installed-client tldr discovery roots; it does not add
  ManT document roots.
- `LC_ALL`, `LC_MESSAGES`, `LANGUAGE`, `LANG`: Select localized manual sources
  and translated tldr pages before English fallback.
- `PATHEXT`: On Windows, order executable suffix fallback for extensionless
  registered-document and native-manual queries.
- `HOME`: On Unix, supply conventional document, manual, and cache locations
  when their XDG overrides are absent.
- `APPDATA`: Select the per-user ManT data root on Windows.
- `LOCALAPPDATA`: Select ManT and installed-client cache roots on Windows.
- `USERPROFILE`: Supply the default Windows manual root and compatible
  installed-client tldr cache locations.

## General

- `--ui`: Require the interactive reader instead of automatic terminal detection.
- `-h`, `--help`: Show command help and exit.
- `-V`, `--version`: Show the installed ManT version and exit.

## Exit Status

`0` indicates success, `2` indicates invalid input or usage, and `1` indicates
an operational failure.

## See Also

The online [protocol reference](https://github.com/BryanHeBY/ManT/blob/main/docs/protocol.md)
documents the JSON and MCP contracts used by external integrations. The
[document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md)
defines `sources.toml` and its update lifecycle.
