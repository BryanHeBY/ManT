<!-- mant:tldr:start -->
# mant

> Turn local manual pages and cross-platform Markdown into structured documents.

- Read a complete manual interactively:

`mant {{name}}`

- Inspect a manual outline:

`mant {{name}} --outline [sections]`

- Retrieve one section from that outline:

`mant {{name}} --node {{path}}`

- Explain one option directly:

`mant {{name}} --explain={{option}}`

- Read a local Markdown file:

`mant --input {{path/to/document.md}}`

- Read Markdown from standard input:

`cat {{path/to/document.md}} | mant --input - --input-format markdown`

- Update configured document sources:

`mant --update-docs`

- Preview removal of sources absent from the configuration:

`mant --prune-docs --dry-run`

- Diagnose the local installation without changing it:

`mant --doctor`
<!-- mant:tldr:end -->

# mant

## Name

`mant` — read and query structured manual pages and cross-platform Markdown.

## Synopsis

```text
mant <SELECTOR> [OPTIONS]
mant <MAN_SECTION> <NAME> [OPTIONS]
mant --input <PATH|-> [--input-format FORMAT] [OPTIONS]
mant --list [FILTERS]
mant --find PATTERN [FILTERS]
mant --request-json [--format FORMAT] [--compact]
mant --doctor [--format text|json] [--compact]
mant --schema CONTRACT [--compact]
mant --update-docs [--compact]
mant --prune-docs [--dry-run] [--compact]
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

## Document Discovery

| Invocation | Behavior |
| --- | --- |
| `--list` | List documents grouped by configured source or native manual section. |
| `--find PATTERN` | Filter document names and emit one stable record per match. |
| `--kind KIND` | Restrict discovery to `markdown` or `manual`. |
| `--source SOURCE` | Restrict Markdown discovery to one configured source. |
| `--man-section MAN_SECTION` | Restrict discovery to one exact native manual category. |
| `--no-pager` | Print `--list` or `--find` text directly even on a terminal. |

Discovery uses a case-insensitive literal substring by default. `--find` also
accepts `--regex` and `--case`; `--limit` and `--offset` apply deterministic
pagination. Plain `--find` output is tab-separated as
the canonical catalog path and `kind`, while `--format json` returns
`mant.catalog/v7`. `--list` groups the same hierarchy beneath `documents`,
`sources/SOURCE`, or `manual/SECTION`.

When stdin and stdout are terminals, discovery text longer than the terminal
height opens in the built-in pager. It supports mouse scrolling, ordinary
less-style movement, and `/` search. Short results print directly. Pipelines,
redirection, `TERM=dumb`, `--format json`, and `--no-pager` never enter the
pager.

```sh
mant --list
mant --find process --source pwsh7
mant --find '^git' --regex --kind manual
mant --list --man-section 3 --format json
```

## Input

Input is resolved before parsing. An ordinary selector first checks the user's
hierarchical `documents` tree. Configured installed sources then compete with
the native manual index, whose priority is `0`: positive source priorities win,
the native manual wins a zero tie, and non-positive sources provide fallback.
Sources within either side are ordered by descending priority and ascending
bytewise name. The configured-source default is `1`. Linux uses
`${XDG_DATA_HOME:-$HOME/.local/share}/mant`, macOS uses
`~/Library/Application Support/ManT`, and Windows uses `%APPDATA%\ManT` as its
data root. Physical filesystem paths are never inferred from positional
selectors; use `--input PATH` explicitly.

Registered `.md` and `.markdown` files retain their extension-free relative
paths. Exact paths win before unique component suffixes; ambiguous suffixes are
reported with their candidates. Complete selectors use
`documents/PATH`, `sources/SOURCE/PATH`, or `manual/SECTION/NAME`. Root
documents always win; source priority, the native-manual zero baseline, and
source name resolve remaining duplicates in the order above; `.md` wins over
`.markdown` for one logical path.
`--source NAME` selects exactly one configured Git or archive source.
`--manual` or `--man-section` selects the full document from native manuals and
cannot be combined with `--source`. `--manual` also excludes the independent
quick-reference channel; `--man-section` alone does not. A manual category is
distinct from a heading or other node inside the loaded document, which is
selected with `--node`.

`--input-format auto|markdown|roff` defaults to `auto` for files and infers the
parser from the filename suffix. The roff loader accepts plain, gzip, and zstd
sources and detects their compression safely. Stdin requires `markdown` or
`roff` explicitly. Direct input may use any OS path but does not follow
redirect-only `.so` pages; those require an indexed MANPATH root.

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

An unqualified name with pages in several native categories follows `MANSECT`
when that variable contains a colon-separated order. Without it, ManT uses the
stable man-db order `1:1p:n:l:8:3:3p:0:0p:2:3type:5:4:9:6:7`; categories absent
from that list follow in lexical order. An explicit `--man-section`, leading
section argument, or `manual/SECTION/NAME` catalog path always selects exactly
that category.

The accepted section spellings are deliberately unambiguous:

```sh
mant 8 btrfs
mant 'btrfs(8)'
mant btrfs --man-section 8
mant manual/8/btrfs
```

A dotted selector such as `btrfs.8` is an exact logical document name, not a
manual shorthand. This preserves Markdown names, dotted executable names, and
tldr collision pages such as `command.1` without a context-dependent guess.
Use `--input PATH` for a physical roff file.

Windows uses `%USERPROFILE%\.local\share\man` as its conventional user root
and accepts additional roots through `MANPATH` or `MANT_MANPATH`.

- `--man-section MAN_SECTION`: Select the full document from one exact native
  manual category such as `1` or `3p`. In an ordinary combined query, a selected
  section `1` or `8` family page may still receive its command quick reference.
- `--manual`: Require only a native manual instead of registered Markdown with
  the same name or an attached quick reference.
- `--tldr`: Print only the highest-precedence embedded or cached quick
  reference and permit it when no full document exists. Personal documents
  precede positive-priority sources, the cached tldr baseline at priority `0`,
  and then non-positive sources. Markdown without an embedded quick reference
  does not participate. Combine `--source SOURCE` to select only that source.
  The default is styled terminal text on a color TTY and plain text through a pipe; use
  `--color always|never` or an explicit `--format` to override it. A cached
  tldr entry alone does not satisfy an ordinary document query: ManT reports
  the missing document and suggests this explicit command.
  When combined with `--tldr`, a section qualifier written as `1 NAME`,
  `8 NAME`, `NAME(1)`, `NAME(8)`, or `--man-section` is accepted only for
  command-section families `1` and `8`; it validates the kind of page but is
  not joined into the tldr topic.
- `--source SOURCE`: Require one configured installed Markdown source.

Recoverable parser findings remain structured in JSON output. ManT does not
invoke a host renderer or maintain an alternate HTML parsing path.

For an ordinary combined native-manual query, ManT automatically attaches a
cached tldr page only when the selected manual belongs to section family `1` or
`8`. This rule is identical for an unqualified lookup and an explicitly selected
manual category. Those categories represent user and administration commands;
other native categories do not inherit a same-named command quick reference.
Explicit `--tldr` accepts only those command-family qualifiers.

### Local Roff Trees

Use `MANT_MANPATH` or `MANPATH` to make project-local man or mdoc sources
visible without copying them into a system directory. Each entry names a root;
pages can live directly below it as `widget.1`, or in section directories such
as `man1/widget.1`. The entry must not name an individual source file.

For example, expose `widget.1` as document name `widget`:

```sh
mkdir -p ./project-man/man1
cp ./widget.1 ./project-man/man1/widget.1
MANT_MANPATH="$PWD/project-man" mant widget --man-section 1
```

The equivalent PowerShell setup is:

```powershell
New-Item .\project-man\man1 -ItemType Directory -Force | Out-Null
Copy-Item .\widget.1 .\project-man\man1\widget.1
$env:MANT_MANPATH = (Resolve-Path .\project-man).Path
mant widget --man-section 1
```

`MANT_MANPATH` is a complete ManT-specific override. `MANPATH` also replaces
the derived defaults unless it contains an empty component; an empty component
inserts user/XDG, PATH-derived, and conventional system roots at that point.
This preserves familiar path-list behavior without invoking `man`. Unix uses
colon-separated entries; Windows uses semicolon-separated entries. Its
conventional fallback contains only `%USERPROFILE%\.local\share\man`; other
locations remain explicit.

The index accepts a leaf page symlink whose target is a regular file, including
one outside the configured root. It does not traverse directory symlinks or
index broken links. If that leaf is a redirect-only `.so` page, its target is
resolved from the symlink's logical location and must remain inside the
configured root; the same boundary applies to every later redirect.

Logical queries accept `mant widget --man-section 1`, `mant 1 widget`,
`mant 'widget(1)'`, and the canonical `mant manual/1/widget`. For isolated
files, `mant --input ./widget.1` is also valid; only MANPATH queries may resolve
redirect-only `.so` aliases.

## Markdown Documents

Local files and standard input enter the same document, outline, excerpt,
search, and output pipeline as manual pages. The supported subset is
deliberately structural rather than a complete browser-oriented Markdown
implementation.

Registered personal documents may be leaf-file symlinks whose targets are
regular files, including targets outside `documents/`. The link's `.md` or
`.markdown` name defines its logical identity. Directory and broken links are
ignored. Installed source caches never follow links; source updates require
selected Markdown entries to be regular files.

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
breaks, standard links, email links, document-local fragments, and hierarchical
relative `.md` or `.markdown` links remain semantic inline nodes. Relative
document links resolve lexically inside the current registered source;
`..` is accepted only while it remains within that source. Soft source line
breaks become spaces, while hard breaks remain visible.

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

Role-aware entries, including Windows switches, commands, variables, and
environment variables, use an explicit invisible directive before one
complete bullet list. Both the role and matching policy are required:

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
- `-ca.cert`, `-ca.chain`: Preserve exact dotted dash-option names.
- `--config.file=FILE`: Omit a placeholder while preserving the dotted name.

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

<!-- mant:entries role=variable case=insensitive -->
- `$?`: Hold the last PowerShell success state.
- `$LASTEXITCODE`: Hold the last native process exit code.
- `$PSVersionTable`: Describe the running PowerShell version.
```

`role` is `option`, `command`, `variable`, or `environment-variable`; `case`
is `sensitive` or `insensitive`. A variable begins with `$` and may use an
ASCII identifier or one of the special names `$?`, `$$`, and `$^`; `$_` is an
ordinary identifier form. `$env:PATH` remains an environment variable rather
than a general variable. Option declarations may additionally use
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
selectors. Dash options likewise preserve non-empty dotted segments, so
`-ca.cert` and `--config.file=FILE` become `-ca.cert` and `--config.file`.
Arbitrary trailing punctuation, paths, empty dotted segments, and lowercase
equals-value placeholders are rejected instead of being silently truncated.

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
rather than being guessed. Paths and IDs resolve first, followed by exact
aliases and then normalized conveniences such as omitting leading dashes. This
lets an exact command `?` coexist with option spelling `-?`. When aliases at
the same precedence remain ambiguous, the outline carries a source diagnostic
and `entriesComplete: false`; selection returns candidate paths and IDs for a
stable qualification.

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

With a complete logical selector or `--input` file and a terminal on stdin and stdout,
`mant` opens its Ratatui reader. `--ui` requires this mode explicitly. A
projection option or `--format` selects deterministic output instead.

The resizable Outline sidebar forms one tree of addressable nodes: document
sections, options, commands, variables, environment variables, and optional
tldr content. Selecting a node puts its target at the top of the content pane.
After content scrolling settles, the outline follows the first visible
document node.
Underlined references can be followed directly. Markdown fragments and mdoc
`Sx` references jump inside the current page. Relative Markdown links preserve
their hierarchy but resolve only inside the current registered source. mdoc
`Xr`, GNU man `MR`, recognized strong `name(section)` references, and validated
legacy Sphinx empty-destination references resolve to an exact manual section
wherever they occur in a page. Bare `name(section)` prose is not inferred as a
link. HTTP(S) and email links open through the platform's
default handler; local-file and executable URL schemes remain visible but
inert. Successful document and in-page jumps are recorded in a bounded
backward/forward history; missing targets and failed loads leave the current
document and history unchanged.

### Navigation

- `j`, `Down`: Select the next visible node.
- `k`, `Up`: Select the previous visible node.
- `h`, `Left`: Collapse the current branch or select its parent.
- `l`, `Right`: Expand the current branch or select its first child.
- `d`, `PageDown`: Scroll the content down.
- `u`, `PageUp`: Scroll the content up.
- `Ctrl+O`: Open the document finder.
- `Alt+Left`: Return to the previous document or in-page jump.
- `Alt+Right`: Move forward after returning.

Mouse input selects and folds outline nodes, follows in-page,
cross-document, and safe external links, scrolls either pane, drags
scrollbars, and resizes the Outline boundary.

### Page Search

- `Ctrl+F`, `/`: Open the bottom search field.
- `Enter`: Confirm a query or select the next match.
- `n`: Select the next confirmed match.
- `Shift+N`: Select the previous confirmed match.
- `Escape`: Close search and remove match highlighting.

Search runs only after confirmation. Every match stays highlighted while the
active match uses a stronger background and moves into view.

### Document Finder

The `Ctrl+O` window searches the complete local catalog while text is entered
and keeps the displayed result page bounded. Each match shows its configured
Markdown source or exact native manual section. Exact names precede prefixes,
which precede other substring matches. `Up` and `Down` select a result, `Enter`
opens it, and `Escape` closes the window. The Manual menu exposes the finder;
Navigate contains backward/forward history and current-document movement.

### Interface

- `F10`: Open the menu bar.
- `?`: Show keyboard shortcuts.
- `q`: Quit.

The View menu can hide the Outline sidebar. The Navigate and Search menus expose the
same operations as their shortcuts. Terminal setup is restored on normal
exit, errors, and Rust panics.

## Document Selection

- `--outline [DETAIL]`: Print the addressable tree; `entries` is the default and
  `sections` is the compact form. The CLI accepts historical `options` as an
  alias for `entries`.
- `--node SELECTOR`: Return an outline node selected by path, stable ID, or
  semantic-entry alias; repeat the option to select several nodes.
- `--explain ENTRY`: Return exactly one semantic option, command, variable, or
  environment entry.

Outline path `0` and node ID `tldr` designate the reserved tldr outline node,
which contains either an external tldr page or a Markdown document's explicitly
marked tldr preface. It is not a native manual section. Remaining headings use
one-based paths such as `2.3`, and semantic entries use paths such as `2.3/e4`.
`--tldr` selects that reserved node alone and, unlike a general node
projection, explicitly permits a quick reference without a full document. It
uses the normal document priority chain, but considers only Markdown documents
that actually contain an embedded tldr preface; cached tldr occupies the same
priority-zero built-in position as native manuals.

`--node` first recognizes the reserved tldr and document-root selectors, then
resolves exact paths or IDs across sections and entries, exact aliases, and
finally normalized entry shorthands. `--explain` uses the same precedence but
accepts entries only. Duplicate matches at one precedence return deterministic
candidate paths and IDs. Only when no entry matches does an exact section,
root, or tldr selector produce the instruction to use `--node`; consequently a
command alias may have the same spelling as a section ID without being
shadowed.

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
- `--color WHEN`: Select `auto`, `always`, or `never` for human-readable terminal output.
- `--compact`: Omit JSON indentation.
- `--preserve-anchors`: Retain addressable HTML anchors in full-document or excerpt Markdown output.

Clean Markdown output omits internal HTML anchors by default. The `man` format
applies only to a complete native roff manual and emits plain manual content
without an external tldr preface; it rejects Markdown documents and partial
document views. Outline, node, explanation, and search projections default to
text; request Markdown explicitly when its structure is useful. JSON must be
selected explicitly for document queries. `--compact` removes indentation from
JSON queries, schemas, protocol descriptions, doctor reports, and update reports.

Help, diagnostics, the default tldr presentation, and partial-document text
projections share the colour policy. Outline trees, selected nodes,
explanations, and search results use semantic ANSI roles without changing their
visible text. `auto` emits styling only to a capable terminal and respects
`NO_COLOR`, `CLICOLOR`, and `TERM=dumb`; `always` and `never` explicitly
override automatic detection. JSON, Markdown, man-format, MCP, and native
protocol results remain deterministic data rather than decorated terminal
output.

## Diagnostics

`--doctor` performs an offline, read-only inspection of the effective data root,
source configuration and installations, registered documents, bundled
libmandoc, native manual index, conditional Git requirement, and tldr roots. It
does not create directories or lock files, invoke external programs, access the
network, update caches, or remove orphaned sources. Suggested repairs name the
existing explicit maintenance command instead of running it.

Human-readable text is the default. `--format json` returns the independent
`mant.doctor/v1` contract; add `--compact` for one-line JSON, and inspect its
authoritative schema with `mant --schema doctor`. Warnings describe degraded or
actionable local state and exit successfully. An error means a promised runtime
capability is broken and exits with status `1`; invalid usage exits with status
`2`.

Doctor JSON intentionally includes local filesystem paths for diagnosis. It is
a native CLI interface and is not exposed through the read-only MCP server.

## Integration

- `--request-json`: Read one closed `mant.request/v7` object from standard input.
- `--schema CONTRACT`: Print a generated JSON Schema for `doctor`, `request`, `query`, `outline`, `excerpt`, `search`, `catalog`, or `all`.
- `--protocol-version`: Print the exact native protocol versions.
- `--mcp`: Serve read-only ManT tools over silent MCP stdio. Lowering
  diagnostics are omitted; an incomplete entry outline retains only
  `entriesComplete: false`. Inspect details with ordinary CLI or request JSON.

### Protocol Discovery

The current protocol descriptor is:

```json
{
  "protocol": "mant.cli/v7",
  "nativeApiVersion": "7",
  "requestSchema": "mant.request/v7",
  "querySchema": "mant.query/v7",
  "documentSchema": "mant.document/v7",
  "outlineSchema": "mant.outline/v7",
  "excerptSchema": "mant.excerpt/v7",
  "searchSchema": "mant.search/v7",
  "catalogSchema": "mant.catalog/v7"
}
```

Request and response contracts advanced to v7 for shared document discovery
and exact catalog addresses. The independent
`mant.markdown/v1` search-coordinate contract remains unchanged.
Future revisions may advance individual contracts only when their wire shapes
change, so consumers must compare every exact schema identifier. Generated
schemas use JSON Schema Draft 2020-12 and remain the authoritative field-level
definition.
The bundled [mant-protocol(5)](mant-protocol.md)
supplies the complete field reference, examples, compatibility policy,
coordinate rules, and MCP tool contracts.

Standard output is reserved for the requested result. Concise diagnostics use
standard error. `--request-json` accepts the same input and projection model
used by external process integrations. MCP exposes `mant_documents_list`,
`mant_document_outline`, `mant_document_get`, `mant_document_explain`, and
`mant_document_search`. Document tools accept a name and optional source or
manual section, not an arbitrary local path. `mant_documents_list` merges
local Markdown candidates with the native manual index and supports `query`,
`kind`, exact `source` or `manualSection`, and bounded pagination. MCP reads current
local files only; it has no update tool and no cross-call snapshot guarantee.

## Data

- `--update-docs`: Update Git or direct archive sources declared in `sources.toml` and print a complete JSON report.
- `--prune-docs`: Explicitly remove installed source directories absent from `sources.toml`; add `--dry-run` to report exact targets without removal.
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

`sources.toml` lives at the data root. Personal documents remain below
`documents/`; installed source directories remain below `sources/`. See the online
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
- `MANSECT`: Order native manual categories for an unqualified name as a
  colon-separated list; unspecified categories follow after it.
- `PATHEXT`: On Windows, order executable suffix fallback for extensionless
  registered-document and native-manual queries.
- `NO_COLOR`: Disable automatic colour in human-readable terminal output.
- `CLICOLOR`: Set to `0` to disable automatic colour or a nonzero value to
  request it on a terminal.
- `CLICOLOR_FORCE`: Request colour even when automatic terminal detection
  would disable it; an explicit `--color` remains authoritative.
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

[mant-protocol(5)](mant-protocol.md), [mant-ir(7)](mant-ir.md), [mant-markdown(7)](mant-markdown.md), and [mant-roff(7)](mant-roff.md) describe the machine boundary, normalized model, and accepted input languages. The [document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md) defines `sources.toml` and its update lifecycle.
