# ManT JSON Protocol and Schema Reference

This document describes the public machine boundary of `mant`: protocol
discovery, the one-shot JSON request transport, every response projection, the
normalized document model, search coordinates, and the MCP stdio tools.

The generated JSON Schemas emitted by the installed binary are authoritative.
This reference explains how those schemas fit together and how clients should
use them; it is not a substitute for validating against the schemas.

## Contract Discovery

Clients should inspect the executable before sending a request:

```sh
mant --protocol-version
```

The current descriptor is:

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

`--compact` emits the same object without indentation. The command does not
query the manual database, read tldr data, or start the TUI.

### Version Matrix

| Identifier | Scope | Where it appears |
| --- | --- | --- |
| `mant.cli/v7` | One-shot process invocation and stream behavior | `--protocol-version` |
| `7` | Native API generation negotiated by process clients | `nativeApiVersion` |
| `mant.request/v7` | Closed request accepted by `--request-json` | Request `schema` |
| `mant.query/v7` | Complete document plus optional quick reference | Full response `schema` |
| `mant.document/v7` | Source-neutral document AST | `QueryBundle.document.schema` |
| `mant.outline/v7` | Block-free addressable tree | Outline response `schema` |
| `mant.excerpt/v7` | One or more selected nodes | Excerpt response `schema` |
| `mant.search/v7` | Search results and pagination | Search response `schema` |
| `mant.catalog/v7` | Local Markdown and manual-page discovery | Catalog response `schema` |
| `mant.markdown/v1` | Canonical Markdown coordinate space | Search `render.schema` |

The suffixes are contract versions, not the ManT release number. The process
request and response contracts are v7; the independent Markdown coordinate
contract remains `mant.markdown/v1`. Clients must compare every complete
identifier rather than infer one contract from another. Version 7 adds a
shared local-document catalog with exact addresses for every Markdown and
manual-page candidate. Further v7 navigation fields use the same address
model. These additions are not wire-compatible with v6 catalog consumers.

### Compatibility Rules

- Every request and response carries an exact schema discriminator.
- Unknown request fields are rejected at the top level and inside tagged
  `input` and `view` objects.
- Adding a required field, changing a field's meaning, or adding a union
  variant that existing consumers cannot safely handle requires a new schema
  identifier.
- New optional response fields may be added within one schema version.
  Consumers should validate the discriminator strictly while tolerating
  optional fields they do not use.
- Document IDs are unique only inside one returned document. Paths and IDs
  should be rediscovered after the source document changes.
- A process client should probe once per executable, cache the successful
  descriptor, and refuse incompatible identifiers before sending a query.

External clients should follow this policy. ManT's built-in reader uses the
same typed Rust structures in process and therefore does not negotiate its own
protocol version.

## Generated JSON Schemas

ManT generates Draft 2020-12 schemas directly from the Rust Serde types:

```sh
mant --schema request
mant --schema query
mant --schema outline
mant --schema excerpt
mant --schema search
mant --schema catalog
mant --schema all
```

`--schema all` returns an object with the stable keys `request`, `query`,
`outline`, `excerpt`, `search`, and `catalog`. `--compact` is accepted by all schema
commands.

| Catalog key | Root title | Root `$id` |
| --- | --- | --- |
| `request` | `QueryRequest` | `urn:mant:request:v7` |
| `query` | `QueryBundle` | `urn:mant:query:v7` |
| `outline` | `QueryOutline` | `urn:mant:outline:v7` |
| `excerpt` | `QueryExcerpt` | `urn:mant:excerpt:v7` |
| `search` | `QuerySearch` | `urn:mant:search:v7` |
| `catalog` | `DocumentCatalog` | `urn:mant:catalog:v7` |

The request schema is generated for deserialization, while response schemas
are generated for serialization. This distinction matters because input
objects are closed and defaults may be applied while decoding, whereas
optional/default response fields are commonly omitted by the serializer.

Field names use `camelCase`. Tagged union discriminators and enum values use
`kebab-case`. Rust unsigned integer formats such as `uint16` and `uint32`
remain annotations; their numeric bounds are also present in the schema.

A client can capture and validate the request contract without a source
checkout:

```sh
mant --schema request > mant-request.schema.json
mant --schema all --compact > mant-schemas.json
```

## Document Catalog

The native CLI and MCP server use one `mant.catalog/v7` discovery contract:

```sh
mant --list
mant --find process
mant --find '^git' --regex --kind manual --format json
```

`--list` renders the hierarchy rooted at `documents`, `sources/<source>`, and
`manual/<section>`. `--find` emits tab-separated canonical catalog paths and
document kinds by default. JSON output contains a flat, paginatable `documents`
array; each row has an exact `address`, stable `catalogPath`, and descriptive
local `sourcePath`.

Markdown addresses distinguish the root `documents` directory from every
configured source. Manual addresses contain both name and exact section, so
shadowed Markdown candidates and multiple manual sections remain independently
selectable. Literal matching is case-insensitive by default; exact paths or
leaf names rank before component suffixes, prefixes, and other substrings. A
pattern containing `/` additionally matches the complete canonical path.
Regex and case policies use the same
values as document-content search.

## One-Shot Process Transport

The stable machine invocation is:

```sh
mant --request-json --format json --compact
```

The process reads exactly one UTF-8 JSON object from standard input. Request
input is bounded to 65,536 bytes. It writes exactly one selected JSON
projection to standard output on success and reserves standard error for
concise diagnostics.

One invocation handles one request and then exits. This keeps the boundary
simple, isolates native parser failures, and lets callers apply ordinary
process timeouts. The built-in reader bypasses this external transport and
operates on one in-memory document.

### Exit Status

| Status | Meaning | Stream behavior |
| --- | --- | --- |
| `0` | Request succeeded | Projection on stdout |
| `2` | Invalid invocation, JSON, schema, selector, or search input | Diagnostic on stderr |
| `1` | Operational failure such as source lookup or parsing failure | Diagnostic on stderr |

Fatal failures do not return a partial JSON error envelope. Recoverable parser
findings belong to `document.diagnostics`, `outline.diagnostics`, or
`excerpt.diagnostics`.

### Source Resolution Policy

Manual pages have one parser path: ManT performs bounded reads, decompression,
and constrained redirect-only `.so` alias resolution, then gives plain roff
bytes to `libmandoc-rs` with includes denied. Renderer selection is
deliberately absent from `mant.request/v7`. This native-manual source family is
available on Linux, macOS, and Windows through the same owned AST boundary.

For ordinary CLI arguments, `mant NAME --manual` bypasses registered Markdown
with the same name and requires only readable native manual content, without an
attached tldr quick reference. `--section` has the same exclusivity, while
`--tldr` is CLI shorthand for the existing reserved `tldr` node projection. A
request JSON client can select the same manual source unambiguously by supplying
its discovered `section`; sections apply only to native manuals and therefore
bypass registered Markdown.

## Request Contract

Every request has three required fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `schema` | Exact string | Must be `mant.request/v7` |
| `input` | `QueryInput` union | Logical document selector or explicit local input file |
| `view` | `QueryView` union | Full, outline, excerpt, explain, or search projection |

### Input Variants

An unqualified selector first checks the singular per-user `documents` tree,
then installed sources configured by `sources.toml` in descending
`priority` and ascending bytewise source-name order, then the native manual
index. Linux uses
`${XDG_DATA_HOME:-$HOME/.local/share}/mant`, macOS uses
`~/Library/Application Support/ManT`, and Windows uses `%APPDATA%\ManT` as the
data root. Regular `.md` and `.markdown` files are registered recursively by
extension-free relative path; symbolic links are ignored. Exact paths precede
unique component suffixes, and collisions are reported explicitly:

```json
{
  "kind": "document",
  "selector": "printf",
  "section": "3"
}
```

The document input also accepts an optional `source` string. It selects exactly
one configured source and bypasses root Markdown and manuals:

```json
{
  "kind": "document",
  "selector": "printf",
  "source": "team"
}
```

`source` and `section` are mutually exclusive. A missing document in an
explicit source is an error rather than a fallback.

On Windows only, an extensionless document name is tried exactly and then with
each suffix from `PATHEXT` in order. Thus `cargo` can resolve a registered
`cargo.exe.md` or native `cargo.exe` manual, while an explicit `cargo.exe`
never expands further. Canonical document filenames should retain the suffix.
This rule does not inspect the parent shell or PowerShell version; `.ps1` is
eligible only when `PATHEXT` contains it. An unset or empty `PATHEXT` uses
`.COM`, `.EXE`, `.BAT`, and `.CMD`. Non-Windows platforms use exact names.

`section` is optional and bypasses registered Markdown. The native index reads
`MANT_MANPATH` as a complete root override. Otherwise, a set `MANPATH` replaces
the defaults except where an empty component inserts them; an unset `MANPATH`
uses platform conventions. Unix derives user/XDG, PATH, and system roots;
Windows defaults only to `%USERPROFILE%\.local\share\man`.
A root may contain `tool.1` directly or a hierarchy such as
`project-man/man1/tool.1`; both become the logical catalog address
`manual/1/tool`. Raw, gzip, and zstd sources are indexed because those are the
formats ManT's bounded input layer decodes before parsing. A standalone `.1`
path belongs to the explicit `file` input variant rather than manual discovery.

The native index accepts a leaf page symlink when its target is a regular file,
including a target outside the indexed root. It does not traverse directory
symlinks or register broken links. Redirect-only `.so` targets are resolved
from the leaf's logical indexed location and must remain inside the canonical
manual root throughout the redirect chain.

A local Markdown or roff file is selected explicitly by path and parser:

```json
{
  "kind": "file",
  "path": "docs/manuals/mant.md",
  "format": "markdown"
}
```

The process request intentionally has no raw `content` variant. Direct
`format` is `auto`, `markdown`, or `roff`; auto uses a file suffix and supports
plain, gzip, and zstd roff. Direct standard input uses
`mant --input - --input-format markdown|roff`, accepts up to 16 MiB, and does
not add embedded raw content to the versioned request schema. Standalone roff
does not follow redirect-only `.so` pages.

### View Variants

| `kind` | Additional fields | Defaults and bounds | Response |
| --- | --- | --- | --- |
| `full` | None | None | `mant.query/v7` |
| `outline` | `detail` | Required: `sections` or `entries` | `mant.outline/v7` |
| `excerpt` | `nodes` | Non-empty string array | `mant.excerpt/v7` |
| `explain` | `entry` | One non-empty semantic path, ID, or alias | `mant.excerpt/v7` |
| `search` | Search fields below | Defaults are applied while decoding | `mant.search/v7` |

Request JSON uses only the canonical outline detail `"entries"`; v7 rejects
`"options"`. The command-line parser alone retains `--outline=options` as a
human-facing alias for `--outline=entries`. Outline v7 responses always emit
`"detail":"entries"` for the semantic-entry form.

Search view fields are:

| Field | Values | Default |
| --- | --- | --- |
| `pattern` | Non-empty UTF-8 string, at most 4,096 bytes | Required |
| `syntax` | `literal`, `regex` | `literal` |
| `case` | `insensitive`, `sensitive`, `smart` | `insensitive` |
| `scope` | `visible`, `markdown` | `visible` |
| `word` | Boolean | `false` |
| `contextLines` | Integer from 0 through 100 | `0` |
| `limit` | Integer from 1 through 10,000 | `100` |
| `offset` | Non-negative integer | `0` |

Regular expressions that match an empty string are rejected. `smart` case
becomes case-sensitive when the pattern contains an uppercase character.
JSON Schema `maxLength` counts Unicode scalar values; the runtime additionally
enforces the documented 4,096-byte UTF-8 pattern limit.

### Complete Request Examples

Request a full manual:

```json
{
  "schema": "mant.request/v7",
  "input": {
    "kind": "document",
    "selector": "printf",
    "section": "3"
  },
  "view": {
    "kind": "full"
  }
}
```

Discover all sections and semantic entries:

```json
{
  "schema": "mant.request/v7",
  "input": {
    "kind": "document",
    "selector": "tar"
  },
  "view": {
    "kind": "outline",
    "detail": "entries"
  }
}
```

Explain one semantic entry directly (sections with the same name are ignored):

```json
{
  "schema": "mant.request/v7",
  "input": {
    "kind": "document",
    "selector": "tar"
  },
  "view": {
    "kind": "explain",
    "entry": "--exclude"
  }
}
```

Retrieve a section and one option by selectors returned from an outline:

```json
{
  "schema": "mant.request/v7",
  "input": {
    "kind": "document",
    "selector": "tar"
  },
  "view": {
    "kind": "excerpt",
    "nodes": [
      "5.4",
      "acls"
    ]
  }
}
```

Search a Markdown document:

```json
{
  "schema": "mant.request/v7",
  "input": {
    "kind": "file",
    "path": "README.md",
    "format": "markdown"
  },
  "view": {
    "kind": "search",
    "pattern": "MCP",
    "syntax": "literal",
    "case": "smart",
    "scope": "visible",
    "word": true,
    "contextLines": 1,
    "limit": 20,
    "offset": 0
  }
}
```

A shell client can send a request without a temporary file:

```sh
printf '%s\n' \
  '{"schema":"mant.request/v7","input":{"kind":"document","selector":"tar"},"view":{"kind":"outline","detail":"entries"}}' \
  | mant --request-json --format json --compact
```

## Full Query Contract

`view.kind = "full"` returns a `QueryBundle`:

| Field | Required | Meaning |
| --- | --- | --- |
| `schema` | Yes | `mant.query/v7` |
| `label` | Yes | Human-readable source label |
| `address` | No | Exact registered Markdown source or manual name and section |
| `document` | No | Normalized `mant.document/v7` document |
| `tldr` | No | Normalized external or embedded quick reference |

A successful runtime result contains useful `document`, `tldr`, or both.
A tldr-only result is possible when a cached quick reference exists but the
manual is unavailable. A Markdown file can provide an embedded quick
reference and a document in the same bundle.

The embedded form is an input-layer extension, not an additional wire shape.
At the physical start of a Markdown source, invisible
`<!-- mant:tldr:start -->` and `<!-- mant:tldr:end -->` comment lines delimit a
tldr-pages-format preface. ManT emits it as the same `TldrDocument` used for
cached pages, so this syntax does not add another schema or response variant.

An abbreviated but structurally valid Markdown result is:

```json
{
  "schema": "mant.query/v7",
  "label": "guide.md",
  "document": {
    "schema": "mant.document/v7",
    "producer": {
      "name": "mant",
      "version": "0.7.0",
      "engine": {
        "name": "pulldown-cmark",
        "version": "0.13"
      }
    },
    "source": {
      "format": "markdown",
      "path": "guide.md"
    },
    "meta": {
      "title": "Guide"
    },
    "sections": []
  }
}
```

The actual `producer.version` is the installed ManT version; clients must not
hard-code the illustrative value above.

## Document AST

`MantDocument` is renderer-neutral. It describes semantics and normalized
layout without exposing libmandoc pointers, roff macro nodes, HTML, or TUI
components.

### Document Envelope

| Field | Meaning |
| --- | --- |
| `schema` | Exact `mant.document/v7` marker |
| `producer` | ManT version and parser engine |
| `source` | Original source format and path |
| `meta` | Normalized title, section, date, volume, OS, architecture, names, and alias target |
| `diagnostics` | Optional recoverable parser findings |
| `blocks` | Optional content before the first addressable section |
| `sections` | Recursive section tree |

`producer.engine` is `libmandoc` for man/mdoc input and `pulldown-cmark` for
Markdown.

`source.format` is one of `man`, `mdoc`, or `markdown`. Temporary decompression
paths never replace the original `source.path`.

Diagnostic levels are `style`, `warning`, `error`, and `unsupported`.
A diagnostic can include a stable code and an original `SourceSpan`.
Recoverable diagnostics do not imply that the returned document is unusable.

### Sections and Source Locations

Each section contains:

- a document-local `id`;
- a visible `title`;
- optional `spacingBeforeLines`;
- semantic `blocks`;
- recursive `children`;
- an optional original `source` span.

`SourceSpan.line` and `SourceSpan.column` are one-based positions in the
original source. `endLine` and `endColumn` are optional because not every
renderer or roff node exposes an exact end location.

Section depth comes from the tree, not a stored heading-level integer.
Section and explicit anchor IDs share one namespace within a document.

### Block Variants

Every block is tagged by `type`:

| `type` | Principal fields | Meaning |
| --- | --- | --- |
| `paragraph` | `children` | Filled prose |
| `preformatted` | `children`, optional `language` | Literal/code display |
| `list` | `kind`, `items`, optional `start`, `compact` | Bullet, ordered, or plain list |
| `definition-list` | `items`, `compact` | Terms with block-capable descriptions |
| `table` | `rows` | Block-capable cells, spans, and alignment |
| `equation` | `value`, `display` | Preserved equation source |
| `vertical-space` | `lines` | Explicit source-requested blank rows |
| `thematic-break` | None | Semantic horizontal break |
| `unsupported` | optional `name`, `text` | Visible source ManT could not structure |

Most content blocks may also carry:

- `layout.indentColumns`;
- `layout.spacingBeforeLines`;
- an original `source` span.

An omitted layout is equivalent to zero indentation and zero leading rows.
Renderers should consume these normalized hints rather than reconstruct roff
spacing.

List item `blocks` can contain nested lists and displays. An ordered list's
`start` is omitted when unavailable. Table cells contain `blocks`;
`columnSpan` and `rowSpan` default to `1`, and `alignment` can be `left`,
`center`, or `right`.

### Semantic Definitions

A definition item contains rendered `terms`, block-capable `description`,
optional `inlineTerm`, optional `spacingBeforeLines`, and an optional
`identity`.

An identity makes one definition addressable:

```json
{
  "id": "option-exclude",
  "role": "option",
  "case": "sensitive",
  "names": [
    "--exclude"
  ]
}
```

Roles are `option`, `command`, `variable`, and `environment-variable`. `case` is
`sensitive` or `insensitive` and controls alias matching without changing
canonical spelling. `names` contains
normalized aliases suitable for `--node`, `--explain`, outline navigation,
and MCP tools. The complete styled term remains in `terms`; consumers should
not rebuild visible text from `names`.

`inlineTerm` is a lowering decision indicating that the term and the first
description line fit the same row. `spacingBeforeLines = null` or an omitted
field inherits the containing list's compactness policy.

### Inline Variants

Inline nodes are tagged by `type`:

| `type` | Fields | Consumer behavior |
| --- | --- | --- |
| `text` | `value` | Render literal text |
| `strong` | `children` | Strong emphasis |
| `emphasis` | `children` | Emphasis |
| `code` | `value` | Inline or preformatted code fragment |
| `external-link` | `uri`, optional `title`, `children` | External destination |
| `email-link` | `address`, `children` | Email destination without synthetic `mailto:` |
| `document-reference` | `name`, optional `fragment`, `children` | Relative hierarchical Markdown document in the current source |
| `manual-reference` | `name`, optional `section`, `children` | Another installed manual |
| `section-reference` | `target`, `children` | Document-local section ID |
| `anchor` | `id` | Zero-width document-local destination |
| `line-break` | None | Explicit hard break |

Visible child content must be preserved even when a consumer cannot activate
a link. `section-reference.target` is a document ID, not a generated Markdown
slug. `document-reference` retains the extension-free relative path derived
from a `.md` or `.markdown` link and is resolved lexically only inside the
current registered source; `..` cannot cross that source boundary.
`manual-reference` comes directly from mdoc `Xr` and GNU man `MR`, or
conservatively from an unambiguous strongly styled `name(section)` pair in a
traditional man page.

### Quick Reference Contract

`QueryBundle.tldr` and tldr excerpt selections use `TldrDocument`:

| Field | Meaning |
| --- | --- |
| `title` | Command name |
| `description` | Normalized description lines |
| `moreInformation` | Optional upstream information URI/text |
| `examples` | Description/command pairs |
| `platform` | tldr platform or `embedded` |
| `language` | Language code or `und` |
| `sourcePath` | Source page identity |
| `origin` | `embedded`, or omitted for community tldr-pages data |

Every example contains the complete `command` and a `commandParts` array.
Command parts are `text` or `placeholder`; the latter lets interactive and
external renderers highlight standard tldr `{{placeholder}}` syntax without
reparsing the command.

## Outline Projection

An outline contains no document blocks. It is intended for cheap discovery
before an agent requests content:

| Field | Meaning |
| --- | --- |
| `schema` | `mant.outline/v7` |
| `detail` | Echoed `sections` or `entries` mode |
| `label` | Query label |
| `source`, `meta` | Optional document identity |
| `diagnostics` | Optional recoverable parser findings |
| `entriesComplete` | Present as `false` only when semantic-entry declarations were rejected or have ambiguous selectors |
| `nodes` | Recursive addressable tree |

Node kinds are:

| `kind` | Path convention | Additional fields |
| --- | --- | --- |
| `tldr` | `0` | Reserved quick reference |
| `document-root` | `root` | Content before the first heading |
| `document-section` | `1`, `1.2`, `1.2.1` | Recursive `children` |
| `document-entry` | `1.2/o3` | `role`, `case`, and normalized `names` |

`detail = "sections"` omits semantic entries. `detail = "entries"` includes
all recognized entry roles. A missing `entriesComplete` field means `true`;
the exceptional `false` value distinguishes a genuinely empty entry outline
from one made incomplete by rejected source declarations.

Paths are convenient human locations. IDs and aliases are better selectors
when nearby section numbering changes. Neither is globally unique across
documents.

An illustrative response is:

```json
{
  "schema": "mant.outline/v7",
  "detail": "entries",
  "label": "tool.md",
  "source": {
    "format": "markdown",
    "path": "tool.md"
  },
  "meta": {
    "title": "Tool"
  },
  "nodes": [
    {
      "kind": "tldr",
      "path": "0",
      "id": "tldr",
      "title": "TLDR QUICK REFERENCE"
    },
    {
      "kind": "document-section",
      "path": "1",
      "id": "options",
      "title": "Options",
      "children": [
        {
          "kind": "document-entry",
          "path": "1/o1",
          "id": "option-help",
          "title": "-h, --help",
          "role": "option",
          "case": "sensitive",
          "names": [
            "-h",
            "--help"
          ]
        }
      ]
    }
  ]
}
```

## Excerpt Projection

`mant.excerpt/v7` returns complete selected content without returning unrelated
sections:

| Field | Meaning |
| --- | --- |
| `schema` | `mant.excerpt/v7` |
| `label` | Query label |
| `producer`, `source`, `meta` | Optional document identity |
| `diagnostics` | Relevant recoverable findings |
| `selections` | Selected content in source order |

Selection kinds are:

- `tldr`, containing one complete `TldrDocument`;
- `document-root`, containing root `blocks`;
- `document-section`, containing a complete section subtree and breadcrumbs;
- `document-entry`, containing one complete definition item and breadcrumbs.

Selectors may be outline paths, document IDs, or semantic aliases. Overlapping
selections are deduplicated, and source order is preserved. Selecting a
section includes its descendants. Breadcrumbs identify ancestors without
copying their blocks.

The `excerpt` view and `--node` first recognize reserved root and tldr
selectors, then resolve exact paths or IDs across all sections and entries,
exact semantic aliases, and finally normalized shorthands. The `explain` view
uses the same precedence but accepts entries only. Exact aliases therefore win
over conveniences such as omitting leading option dashes or an `$env:` prefix.
Only when no entry matches does an exact section, root, or tldr selector return
an entry-required error. A same-named section therefore cannot shadow an
option, command, variable, or environment variable. Repeated matches at one
precedence are errors rather than first-match selections; diagnostics and
runtime errors return candidate paths and IDs in source order.

Direct `mant --explain=--exclude` and MCP `mant_document_explain` reuse this
contract, then require the result to contain exactly one `document-entry`.
There is intentionally no separate explanation response schema.

## Search Projection

`mant.search/v7` searches one canonical full CommonMark render and returns
both structural locations and rendered coordinates.

### Result Envelope

| Field | Meaning |
| --- | --- |
| `schema` | `mant.search/v7` |
| `label`, `source`, `meta` | Source identity |
| `query` | Fully normalized search settings |
| `render` | Coordinate-space descriptor |
| `total` | All matching occurrences before pagination |
| `returned` | Number of matches in this page |
| `offset` | Echoed pagination offset |
| `truncated` | Whether more matches remain |
| `nextOffset` | Next deterministic offset when truncated |
| `matches` | Exact occurrences |

`query` always echoes all defaults, even when the request omitted them.
A no-match search is successful and returns `total = 0` with an empty
`matches` array.

### Coordinate Model

The render descriptor is currently:

```json
{
  "schema": "mant.markdown/v1",
  "format": "markdown",
  "scope": "full",
  "lineBase": 1,
  "columnBase": 1,
  "lineCount": 900
}
```

`lineCount` is document-dependent. Match `markdown.startByte` and `endByte`
form a half-open UTF-8 byte range in that exact canonical Markdown.
`startLine`, `startColumn`, `endLine`, and `endColumn` are one-based human
coordinates. Columns count Unicode scalar values rather than UTF-8 bytes.

`scope = "visible"` changes what can match, but coordinates still point into
the canonical Markdown. `scope = "markdown"` also allows matches in markup.

Each match includes:

- a one-based global `ordinal` that is not reset by pagination;
- the nearest reusable `node` accepted by excerpt selection;
- an optional containing `section`;
- exact `matchedText`;
- its Markdown range;
- an optional original `source` span;
- a human-readable `preview`;
- optional full Markdown context lines.

The node union uses the same `tldr`, `document-root`, `document-section`, and
`document-entry` identities as outlines.

A complete no-match response is:

```json
{
  "schema": "mant.search/v7",
  "label": "tar",
  "query": {
    "pattern": "definitely-not-present",
    "syntax": "literal",
    "case": "insensitive",
    "scope": "visible",
    "word": false,
    "contextLines": 0,
    "limit": 100,
    "offset": 0
  },
  "render": {
    "schema": "mant.markdown/v1",
    "format": "markdown",
    "scope": "full",
    "lineBase": 1,
    "columnBase": 1,
    "lineCount": 900
  },
  "total": 0,
  "returned": 0,
  "offset": 0,
  "truncated": false,
  "matches": []
}
```

## MCP Stdio Transport

`mant --mcp` is a long-running Model Context Protocol server over standard
input and output. It is an alternate transport over the same `mant-core`
queries, not `mant.cli/v7` framing and not a separate document model.

The server uses JSON-RPC 2.0 newline-delimited MCP stdio messages. One input
line is limited to 8 MiB. Standard output is exclusively MCP traffic;
standard error is deliberately silent. Lowering diagnostics are omitted from
MCP outlines and excerpts, while tool failures use structured MCP error results
and fatal transport failures use a non-zero process status. An incomplete MCP
entry outline retains only `entriesComplete: false`; diagnose the exact source
finding through ordinary CLI or request JSON output. There is no HTTP listener
and there are no mutation tools. Each call reads the local files visible at
that time; MCP does not invoke Git or HTTP, update sources, or promise one fixed
snapshot across calls.

MCP protocol versions are negotiated by the standard `initialize` exchange.
With the current runtime, a client requesting `2025-11-25` receives:

```json
{
  "protocolVersion": "2025-11-25",
  "capabilities": {
    "tools": {}
  },
  "serverInfo": {
    "name": "mant",
    "version": "0.7.0"
  },
  "instructions": "Read locally installed Markdown documents and manual pages by name. Use mant_documents_list for discovery, optionally select a configured source, then call mant_document_outline before retrieving IDs, paths, or aliases. Files may change between calls; this server does not update sources."
}
```

The installed version is reported dynamically. MCP clients should use the
negotiated `initialize` result rather than treating the example's protocol or
server version as a permanent ManT constant.

### Tools

`tools/list` returns generated input and output schemas for exactly five
read-only tools:

| Tool | Required input | Optional input | Output |
| --- | --- | --- | --- |
| `mant_documents_list` | None | `query`, `kind`, `source`, `section`, `limit`, `offset` | Paginated document catalog |
| `mant_document_outline` | `name` | `source` or `section`; `detail`, default `entries` | `mant.outline/v7` |
| `mant_document_get` | `name`, non-empty `nodes` | `source` or `section` | `mant.excerpt/v7` |
| `mant_document_explain` | `name`, `entry` | `source` or `section` | `mant.excerpt/v7` |
| `mant_document_search` | `name`, `pattern` | `source` or `section`, plus search settings | `mant.search/v7` |

Every tool is annotated read-only, non-destructive, and
closed-world. Document tools resolve one name through root Markdown,
configured installed sources, and then the native manual index. They do not
accept arbitrary file paths. `source` selects one configured source; `section`
selects a manual; the two selectors cannot be combined.

Discover both registered Markdown and section-qualified manual pages with:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "mant_documents_list",
    "arguments": {
      "query": "printf",
      "kind": "manual",
      "limit": 100,
      "offset": 0
    }
  }
}
```

The catalog response reports `total`, `returned`, `offset`, `truncated`, an
optional `nextOffset`, and `documents`. Each row has an exact tagged `address`,
a stable canonical `catalogPath`, and a descriptive local `sourcePath`.
Markdown addresses carry their relative path plus a `documents` or named
`source` origin; manual addresses carry the exact name and section. `query`
matches leaf names and relative paths case-insensitively by default; a query
containing `/` also matches canonical paths. Shadowed Markdown candidates
remain in the catalog. `limit` defaults to 100 and is capped at 10,000.

An outline tool call is:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "mant_document_outline",
    "arguments": {
      "name": "tar",
      "detail": "entries"
    }
  }
}
```

Inspect each returned `document-entry` for its `role`, `case`, `names`, `path`,
and `id`, then request exactly one entry:

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "mant_document_explain",
    "arguments": {
      "name": "reg.exe",
      "entry": "query"
    }
  }
}
```

The same tool accepts option aliases such as `/query` and environment aliases
such as `PATH` or `$env:PATH`. Matching follows the entry's declared case
policy. If an alias occurs more than once, the tool error names every candidate
path and ID; repeat the call with one of those qualifiers, for example
`"entry":"2/o1"` or `"entry":"command-query"`. Tool-error text is a
human-readable diagnostic rather than a separately versioned structured
schema, so automated clients should prefer outline-provided paths and IDs and
must not depend on parsing its prose.

A structure-aware search tool call is:

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "mant_document_search",
    "arguments": {
      "name": "tar",
      "pattern": "--acls",
      "syntax": "literal",
      "case": "insensitive",
      "scope": "visible",
      "contextLines": 1,
      "limit": 20,
      "offset": 0
    }
  }
}
```

Tool outputs are the same versioned projection objects described above.
Protocol-level or validation failures use standard MCP error results rather
than inventing a ManT error schema.

## Client Implementation Checklist

1. Resolve the intended `mant` executable.
2. Run `mant --protocol-version --compact` and require compatible identifiers.
3. Obtain `mant --schema request` and the expected response schema, or use a
   schema catalog pinned with the executable.
4. Construct a closed `mant.request/v7` object.
5. Spawn `mant --request-json --format json --compact`.
6. Write one UTF-8 request and close stdin.
7. Drain stdout and stderr concurrently and apply a timeout.
8. Require status `0`, parse exactly one JSON value, and validate its exact
   response discriminator.
9. Use outline paths, IDs, and aliases only within their source document.
10. For search, interpret offsets against `mant.markdown/v1`, not the original
    roff or Markdown input.

For long-lived agent integration, use `mant --mcp`, perform standard MCP
initialization, and consume the generated tool schemas from `tools/list`.
