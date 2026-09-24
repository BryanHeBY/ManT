# mant-protocol

## Name

mant-protocol — shared interaction contracts and presentations for ManT

## Description

This document describes the unified interaction boundary of `mant`: Rust host
DTOs, protocol discovery, the one-shot JSON request transport, every response
projection, the normalized document model, search coordinates, and compact MCP
stdio presentation. A projection may remain in memory, cross a versioned JSON
transport, or be rendered into a bounded agent-facing result.

The generated JSON Schemas emitted by the installed binary are authoritative
for structured JSON boundaries. MCP advertises closed input schemas through
`tools/list`, but its successful outputs are plain text or CommonMark rather
than native response envelopes. This reference explains how the boundaries fit
together and how clients should use them; it is not a substitute for validating
structured input against the appropriate schema.

The Rust package owns DTOs, validation, schema generation and stable semantic
labels. `mant-query` produces these projections from existing IR; `mant-render`
formats existing results as text or CommonMark without re-running queries or
loading documents. The CLI and MCP host compose those capabilities. Changing
their crate ownership does not change the wire discriminators below.

Protocol projections reuse selected semantic types from `mant-ir`, including
blocks, sections, inline nodes, entry facts, logical document
addresses, metadata, diagnostics, and tldr content. These types are the
wire-bearing semantic subset. Their Serde representations are locked by a
complete structural Schema snapshot; changing one under an unchanged schema
discriminator fails CI. Descriptions and titles may improve without changing
the structural contract.

## Contract Discovery

Clients should inspect the executable before sending a request:

```sh
mant --protocol-version
```

The current descriptor is:

```json
{
  "protocol": "mant.cli/v0.12",
  "nativeApiVersion": "0.12",
  "requestSchema": "mant.request/v0.12",
  "querySchema": "mant.query/v0.12",
  "documentSchema": "mant.document/v0.12",
  "outlineSchema": "mant.outline/v0.12",
  "excerptSchema": "mant.excerpt/v0.12",
  "explanationSchema": "mant.explanation/v0.12",
  "searchSchema": "mant.search/v0.12",
  "scopeRequestSchema": "mant.scope-request/v0.12",
  "scopeQuerySchema": "mant.scope-query/v0.12",
  "catalogSchema": "mant.catalog/v0.12"
}
```

`--compact` emits the same object without indentation. The command does not
query the manual database, read tldr data, or start the TUI.

### Version Matrix

| Identifier | Scope | Where it appears |
| --- | --- | --- |
| `mant.cli/v0.12` | One-shot process invocation and stream behavior | `--protocol-version` |
| `0.12` | Native API release line negotiated by process clients | `nativeApiVersion` |
| `mant.request/v0.12` | Closed request accepted by `--request-json` | Request `schema` |
| `mant.query/v0.12` | Complete document plus optional quick reference | Full response `schema` |
| `mant.document/v0.12` | Source-neutral document response | `QueryBundle.document.schema` |
| `mant.outline/v0.12` | Block-free addressable tree | Outline response `schema` |
| `mant.excerpt/v0.12` | One or more selected nodes | Excerpt response `schema` |
| `mant.explanation/v0.12` | Independent bounded semantic evidence | Explanation response `schema` |
| `mant.search/v0.12` | Search results and pagination | Search response `schema` |
| `mant.scope-request/v0.12` | Bounded document-set search or explanation | Scope request `schema` |
| `mant.scope-query/v0.12` | Resolved graph and grouped projection | Scope response `schema` |
| `mant.catalog/v0.12` | Local Markdown and manual-page discovery | Catalog response `schema` |
| `mant.markdown/v1` | Canonical Markdown presentation coordinates | Search `render.schema` |
| `mant.doctor/v1` | Read-only local installation diagnostics | Doctor report `schema` |

The native query family follows ManT's pre-stable minor release line:
ManT 0.12.x uses `v0.12`, and patch releases remain backward compatible. They
may add documented optional response fields, but never change requests,
required fields, tagged unions, or existing field semantics. The
independent Markdown coordinate and doctor contracts remain
`mant.markdown/v1` and `mant.doctor/v1`. Clients must still compare complete
identifiers rather than infer compatibility between independent families.
The `mant-protocol` Rust crate has its own semver; upgrading that package does
not by itself select a new wire discriminator.

The former bare `v1` through `v7` query schemas were experimental pre-stable
contracts. ManT 0.8 removes them rather than carrying compatibility code; a
request using one is rejected as an unknown schema. Their definitions remain
available from the corresponding historical releases and tags. Once the
native protocol is stable, its first stable release line will be named
`v1.0`.

The 0.8 line added a shared local-document catalog with exact addresses for
every Markdown and manual-page candidate. It also separates producer metadata
from the in-memory IR, gives selectors typed `NodePath` semantics, uses typed
node IDs and exact byte ranges, represents every link through one tagged
target union, and gives excerpt and search results one common `OutlineTrail`.

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
mant --schema explanation
mant --schema search
mant --schema scope-request
mant --schema scope-query
mant --schema catalog
mant --schema doctor
mant --schema tldr-update
mant --schema all
```

`--schema all` returns an object with the stable keys `request`, `query`,
`outline`, `excerpt`, `explanation`, `search`, `scope-request`, `scope-query`, `catalog`,
`doctor`, and `tldr-update`. The latter two retain their independent contract
families even though the catalog exposes them together with the release-aligned
document schemas. `--compact` is accepted by all schema commands.

| `--schema` value | Root title | Root `$id` |
| --- | --- | --- |
| `request` | `QueryRequest` | `urn:mant:request:v0.12` |
| `query` | `QueryBundle` | `urn:mant:query:v0.12` |
| `outline` | `QueryOutline` | `urn:mant:outline:v0.12` |
| `excerpt` | `QueryExcerpt` | `urn:mant:excerpt:v0.12` |
| `explanation` | `QueryExplanation` | `urn:mant:explanation:v0.12` |
| `search` | `QuerySearch` | `urn:mant:search:v0.12` |
| `scope-request` | `ScopeQueryRequest` | `urn:mant:scope-request:v0.12` |
| `scope-query` | `ScopeQueryResponse` | `urn:mant:scope-query:v0.12` |
| `catalog` | `DocumentCatalog` | `urn:mant:catalog:v0.12` |
| `doctor` | `DoctorReport` | `urn:mant:doctor:v1` |
| `tldr-update` | `TldrCacheUpdate` | `urn:mant:tldr-update:v1` |

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

## Doctor Report

`mant --doctor --format json` emits `mant.doctor/v1`, a native, offline snapshot
of the effective local installation. The report contains a platform/version
environment, an overall `healthy`, `warning`, or `error` outcome, ordered checks,
and an aggregate summary. Each check has a stable `code`, `ok`, `info`,
`warning`, or `error` status, a concise `message`, optional logical `subject`,
bounded `details`, and optional `remediation`. Warnings retain exit status `0`;
any error produces exit status `1`.
For `sources.installation`, an `ok` result means the installed metadata,
document count, and active local configuration agree. It includes an explicit
detail that remote freshness was not checked; doctor never contacts a Git or
archive origin.

| Check field | Meaning |
| --- | --- |
| `code` | Stable machine-readable check identifier |
| `subject` | Optional configured source or other logical subject |
| `status` | `ok`, `info`, `warning`, or `error` |
| `message` | Concise human-readable outcome |
| `details` | Ordered bounded evidence strings |
| `remediation` | Optional command or corrective action |

```json
{
  "schema": "mant.doctor/v1",
  "outcome": "warning",
  "checks": [
    {
      "code": "sources.installation",
      "subject": "team",
      "status": "warning",
      "message": "installed source is invalid or unreadable",
      "details": ["installed metadata records 20 documents but 19 are present"],
      "remediation": "mant --update-docs"
    }
  ]
}
```

The abbreviated example omits the required `producer`, `environment`, and
`summary` objects only to focus on the check vocabulary; the generated schema
is authoritative for a complete report.

Doctor may expose physical filesystem paths because local provenance is needed
to repair an installation. It omits configured repository and archive URLs and
is not an MCP tool, so MCP consumers continue to see logical document identities
rather than host filesystem layout. The command never creates directories or
locks, invokes external programs, contacts the network, updates caches, or
removes data.

## TLDR Cache Update Result

`mant --update-tldr` emits `mant.tldr-update/v1`. `action` is `cloned` or
`updated`; optional `cacheDir`, `client`, `output`, and `revision` retain the
locally available update evidence. This native maintenance contract is not
part of the read-only MCP surface. Its schema is available with
`mant --schema tldr-update`.

## Document Catalog

The native CLI and MCP server share one catalog query and logical projection.
Structured CLI JSON serializes it as `mant.catalog/v0.12`; MCP renders a bounded
text view of the same canonical identities:

```sh
mant --list
mant --find process
mant --find '^git' --regex --kind manual --format json
```

`--list` renders the hierarchy rooted at `documents`, `sources/<source>`, and
`manual/<section>`. `--find` emits tab-separated canonical catalog paths and
document kinds by default. JSON output contains a flat, paginatable `documents`
array; each row has one exact logical `address`. Its canonical catalog path is
derived from that address rather than duplicated in the wire value. Physical
paths are intentionally absent from discovery results.

Every response echoes the normalized `query` and carries `coverage` separately
from the name-match `total`. `coverage.scopeTotal` counts documents after the
kind, source, and exact manual-section filters but before the name pattern. A
zero `scopeTotal` therefore means that the requested namespace is not indexed;
a positive `scopeTotal` with zero matches means that the namespace was searched
and the name was absent. `manualSections` retains exact categories such as
`2const`, `2type`, and `3pm`; numeric base sections do not silently include
their extensions.

Markdown addresses distinguish the root `documents` directory from every
configured source. Manual addresses contain both name and exact section, so
shadowed Markdown candidates and multiple manual sections remain independently
selectable. Literal matching is case-insensitive by default; exact paths or
leaf names rank before component suffixes, prefixes, and other substrings. A
case-faithful spelling ranks before a match that reaches the same relevance
tier only through case folding; matching itself remains case-insensitive. A
pattern containing `/` additionally matches the complete canonical path.
Regex and case policies use the same
values as document-content search. The native process may page long text
results when it owns an interactive terminal; this presentation-only behavior
never changes JSON, redirected output, or the catalog protocol.

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
deliberately absent from `mant.request/v0.12`. This native-manual source family is
available on Linux, macOS, and Windows through the same owned IR boundary.

For ordinary CLI arguments, `mant NAME --manual` bypasses registered Markdown
with the same name and requires only readable native manual content, without an
attached tldr quick reference. A `manualSection` selects the full document from
one exact native category and bypasses registered Markdown, but the default
combined policy may still attach a quick reference when that category belongs
to command family `1` or `8`. `--tldr` selects the reserved `tldr` channel
through the document priority chain and explicitly permits an embedded or
cached tldr-only result. A section `1` or `8` qualifier may validate such a
command query without becoming part of the tldr topic; other categories are a
usage error. A request JSON client uses the same rules by supplying its
discovered `manualSection`.

## Request Contract

Every request has three required fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `schema` | Exact string | Must be `mant.request/v0.12` |
| `input` | `QueryInput` union | Logical document selector or explicit local input file |
| `view` | `QueryView` union | Full, outline, excerpt, explain, or search projection |

### Input Variants

An unqualified selector first checks the singular per-user `documents` tree.
Installed sources configured by `sources.toml` then compete with the native
manual index at priority `0`: positive source priorities win, the native manual
wins a zero tie, and non-positive sources are fallbacks. Sources on either side
use descending `priority` and ascending bytewise source-name order. Omitted
source priorities default to `1`. Linux uses
`${XDG_DATA_HOME:-$HOME/.local/share}/mant`, macOS uses
`~/Library/Application Support/ManT`, and Windows uses `%APPDATA%\ManT` as the
data root. Regular `.md` and `.markdown` files are registered recursively by
extension-free relative path. Personal `documents/` accepts leaf-file symlinks
to regular files, including external targets; their link path supplies the
identity. Directory and broken links are ignored, and managed source caches
never follow links. Exact paths precede unique component suffixes, and
collisions are reported explicitly:

```json
{
  "kind": "document",
  "selector": "printf",
  "manualSection": "3"
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

`source` and `manualSection` are mutually exclusive. A missing document in an
explicit source is an error rather than a fallback.

On Windows only, an extensionless document name is tried exactly and then with
each suffix from `PATHEXT` in order. Thus `cargo` can resolve a registered
`cargo.exe.md` or native `cargo.exe` manual, while an explicit `cargo.exe`
never expands further. Canonical document filenames should retain the suffix.
This rule does not inspect the parent shell or PowerShell version; `.ps1` is
eligible only when `PATHEXT` contains it. An unset or empty `PATHEXT` uses
`.COM`, `.EXE`, `.BAT`, and `.CMD`. Non-Windows platforms use exact names.

`manualSection` is optional and bypasses registered Markdown. The native index reads
`MANT_MANPATH` as a complete root override. Otherwise, a set `MANPATH` replaces
the defaults except where an empty component inserts them; an unset `MANPATH`
uses platform conventions. Linux follows man-db `MANPATH_MAP`,
`MANDATORY_MANPATH`, and `SYSTEM` from `~/.manpath` or the usual system
configuration locations, falling back to mandoc `/etc/man.conf` `manpath`
entries. macOS follows its `$PATH`, active Xcode or Command Line Tools manual
trees, system defaults, then `/etc/man.conf` `MANPATH` plus `MANCONFIG`
fragments; it reads the xcode-select state directly rather than spawning that
tool. Windows has no native convention; ManT reads an optional
`%APPDATA%\ManT\man.conf`. Its Windows-only, case-insensitive source-root
subset accepts `MANPATH`, bounded one-level `MANCONFIG` fragments,
PATH-conditioned `MANPATH_MAP`, and `MANDATORY_MANPATH`; paths accept optional
double quotes and single-pass `%NAME%` environment expansion. Direct primary
and fragment roots precede mapped and mandatory roots, then ManT automatically
checks `%APPDATA%\ManT\man` and the compatible
`%USERPROFILE%\.local\share\man` fallback. Windows environment names use the
platform's case-insensitive semantics, and fragment expansion stops before
traversing patterns beyond its global bound. Invalid directives are omitted;
the native CLI exposes their local file and line through `mant --doctor`,
while structured document results retain logical identities rather than
physical configuration paths. Discovery is read-only and does not invoke a
host command.
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
| `full` | None | None | `mant.query/v0.12` |
| `outline` | `entries`, `root`, `references` | Independent entry/reference summaries by default; optional projection and root selector | `mant.outline/v0.12` |
| `excerpt` | `selectors` | Non-empty node-selector array | `mant.excerpt/v0.12` |
| `explain` | `entry`, optional `options` | Bounded name/form/entry-coordinate/literal evidence | `mant.explanation/v0.12` |
| `search` | Search fields below | Defaults are applied while decoding | `mant.search/v0.12` |

`entries` is a tagged projection. `{"kind":"none"}` emits only section
topology, `{"kind":"summary"}` is the default, and `{"kind":"all"}` emits
the complete nested semantic index. `{"kind":"kinds","kinds":[...]}` retains
the selected entry kinds plus any ancestors required to reach them, pruning
unrelated branches. If no selected entry exists, `nodes` is empty and compact
text/CommonMark presentation reports an explicit zero-match result. Parameter
kinds use objects such as
`{"kind":"parameter","parameterKind":"option"}`. `root`, when present,
is a closed `ContentSelector`: `{"kind":"path","path":"2.3/e4"}` or
`{"kind":"id","id":"option-help"}`. No names, aliases, shorthand, URIs or
fragment activation are accepted. Duplicate IDs return candidates; an exact
path reads one owner. Structural selector values reject controls and exceedances
of 512 UTF-8 bytes; explanation input uses its separate 512-scalar bound.
The former v0.9 `detail` field and
`--outline=entries` syntax are rejected by v0.12.

Search view fields are:

| Field | Values | Default |
| --- | --- | --- |
| `pattern` | Non-empty string, at most 4,096 Unicode scalar values | Required |
| `syntax` | `literal`, `regex` | `literal` |
| `case` | `insensitive`, `sensitive`, `smart` | `insensitive` |
| `word` | Boolean | `false` |
| `contextLines` | Integer from 0 through 100 | `0` |
| `limit` | Integer from 1 through 10,000 | `100` |
| `offset` | Non-negative integer | `0` |

Regular expressions that match an empty string are rejected. Regex patterns
must preserve Unicode mode and UTF-8 character boundaries; byte-oriented forms
that disable Unicode, such as `(?-u:.)`, are rejected before document matching.
`smart` case becomes case-sensitive when the pattern contains an uppercase
character. JSON Schema `maxLength` and runtime validation both count Unicode
scalar values.

### Complete Request Examples

Request a full manual:

```json
{
  "schema": "mant.request/v0.12",
  "input": {
    "kind": "document",
    "selector": "printf",
    "manualSection": "3"
  },
  "view": {
    "kind": "full"
  }
}
```

Discover all sections and semantic entries:

```json
{
  "schema": "mant.request/v0.12",
  "input": {
    "kind": "document",
    "selector": "tar"
  },
  "view": {
    "kind": "outline",
    "entries": {
      "kind": "all"
    }
  }
}
```

Discover option and value entries below one command without materializing
unrelated branches. Substitute the command ID returned by the current outline
for the illustrative `command-set` below:

```json
{
  "schema": "mant.request/v0.12",
  "input": {
    "kind": "document",
    "selector": "bash"
  },
  "view": {
    "kind": "outline",
    "entries": {
      "kind": "kinds",
      "kinds": [
        {"kind": "parameter", "parameterKind": "option"},
        {"kind": "value"}
      ]
    },
    "root": {"kind":"id","id":"command-set"}
  }
}
```

Collect independent evidence. An equal section ID does not shadow a documented
name, and multiple owners are not ambiguity errors. Options default to 50
records, offset zero and a 1 MiB forms/facts/previews/body copy budget:

```json
{
  "schema": "mant.request/v0.12",
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
  "schema": "mant.request/v0.12",
  "input": {
    "kind": "document",
    "selector": "tar"
  },
  "view": {
    "kind": "excerpt",
    "selectors": [
      {"kind":"path","path":"5.4"},
      {"kind":"id","id":"acls"}
    ]
  }
}
```

Search a Markdown document's canonical visible render:

```json
{
  "schema": "mant.request/v0.12",
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
  '{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"outline","entries":{"kind":"all"}}}' \
  | mant --request-json --format json --compact
```

## Bounded Document Scope Contract

`mant.scope-request/v0.12` is a separate closed request rather than an array-valued variant of `mant.request/v0.12`. The separation keeps full, outline, node, tldr, and direct-file queries unambiguously single-document while allowing search and semantic explanation to operate over a linked set.

The request has three fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `schema` | Exact string | `mant.scope-request/v0.12` |
| `scope` | `DocumentScope` | Ordered initial documents plus traversal policy |
| `view` | `ScopeQueryView` | `search` or `explain` |

`scope.documents` contains from 1 through 16 logical selectors. Each selector has the same `selector`, optional `source`, and optional `manualSection` fields as a single document input. Complete catalog paths remain the unambiguous form when initial documents come from different sources or manual sections.

`scope.traversal` is closed and has these fields:

| Field | Default | Bound | Meaning |
| --- | ---: | ---: | --- |
| `followLinks` | `false` | Boolean | Follow typed outbound document links |
| `maxDepth` | `8` | 0 through 32 | Maximum number of link edges followed from an initial document |
| `maxDocuments` | `64` | At least the initial-document count; at most 256 | Distinct loaded documents, including roots |

`maxDepth` and `maxDocuments` are optional and valid only when `followLinks` is `true`; supplying either field while traversal is disabled is rejected. `maxDepth = 0` resolves only the initial documents and follows no links. `maxDepth = 1` additionally resolves their one-hop neighbours.

`LinkTarget::Document` and `LinkTarget::Manual` create edges. Explicit semantic entry destinations and entry-set value domains use the same restricted Markdown/manual reference family and also participate in traversal. Markdown targets resolve relative to the referring document and retain its personal/source namespace; a path escaping that namespace is rejected. Manual links with a section resolve that exact logical address. Page-local, external, and email links are excluded. Plain prose and filename prefixes are never interpreted as edges.

Traversal is breadth-first. Initial selector order comes first, followed by typed links in source order. A canonical `DocumentAddress` supplies cycle detection and deduplication. A document reached by several parents is queried once while `reachedFrom` retains each distinct parent. Missing roots and links appear in `unresolved`; `from` is absent for an initial selector and present for a followed link. A request fails only when no initial document is readable.

`frontier` records each typed logical link that traversal did not follow. Its `limit` is `max-depth`, `max-documents`, or `max-content-bytes`; its `target` remains a `DocumentSelector` because resolving a target can itself exceed the requested bound. Scope resolution retains at most 64 MiB of normalized semantic IR across all loaded documents; this fixed aggregate budget bounds in-memory traversal even when every individual input satisfies its own file-size limit. An initial selector excluded by this aggregate budget is recorded in `unresolved` with no `from`; other roots remain usable, and resolution fails only when the budget or ordinary lookup failures leave no readable initial document. Merely loading a document at `maxDepth` does not create a frontier entry when that document has no outbound typed links. Links from a boundary document to an address already loaded in the scope remain ordinary resolved `edges` rather than false truncation signals.

Identical `unresolved` records are reported once per referring document;
distinct referring documents remain observable. Failed selector resolutions
are cached only within the current scope request, using both resolution policy
and qualified selector. A later request reads installed state again.

Example:

```json
{
  "schema": "mant.scope-request/v0.12",
  "scope": {
    "documents": [
      { "selector": "git" },
      { "selector": "git-lfs" }
    ],
    "traversal": {
      "followLinks": true,
      "maxDepth": 4,
      "maxDocuments": 64
    }
  },
  "view": {
    "kind": "search",
    "pattern": "worktree",
    "syntax": "literal",
    "case": "insensitive",
    "scope": "visible",
    "word": false,
    "contextLines": 1,
    "limit": 20,
    "offset": 0
  }
}
```

The response uses `mant.scope-query/v0.12`. Its `scope` field contains the request, ordered resolved documents, unique edges, optional unresolved targets, and the typed traversal frontier. For `result.kind = "search"`, pagination lives under `result.search`: consumers read `result.search.total`, `returned`, `offset`, `truncated`, `nextOffset`, and `documents`. Each retained document group contains `address`, `depth`, a `render` descriptor, and complete `matches`; visible hits also carry a bounded `contentProjection`. The descriptor identifies the applicable artifact and must not be treated as the authoritative coordinate for a `visible-flow` or `visible-fixed` match. Groups deliberately have no local pagination fields or nested `mant.search/v0.12` envelope. Hit `ordinal` values are one-based in the complete unpaginated scope and therefore remain unique across document groups and result pages. Search-level `truncated` describes occurrence pagination, not document traversal. Limit and offset apply globally, not once per document.

```json
{
  "schema": "mant.scope-query/v0.12",
  "scope": {
    "query": { "documents": [{ "selector": "git" }], "traversal": { "followLinks": false } },
    "documents": [],
    "edges": []
  },
  "result": {
    "kind": "search",
    "search": {
      "schema": "mant.scope-search/v0.12",
      "query": { "pattern": "index", "syntax": "literal", "case": "insensitive", "word": false, "contextLines": 0, "limit": 20, "offset": 0 },
      "total": 0,
      "returned": 0,
      "offset": 0,
      "truncated": false,
      "semanticsComplete": true,
      "coverageByDocument": [],
      "documents": []
    }
  }
}
```

For `result.kind = "explain"`, `result.explanation` is a `ScopeExplanation`.
It owns `query`, `order`, `counts`, aggregate `outcome`, `total`, `returned`,
optional `nextOffset`, independent `truncation`, BFS `documents`, one globally
ordered `evidence` list and `failures`. Each readable document report contains
`address`, `depth`, `label`, optional `sourceContext`, optional `producer`, `diagnostics`,
`semanticsComplete`, `outcome`, `total`, `returned`, `counts` and `truncation`,
even when it found no evidence or contributed nothing to this page. Reports
contain no nested query, cursor or evidence/body. Each flat record is
`{documentIndex, evidence}`: the zero-based index refers to this explanation's
`documents`, not the outer scope graph; its `address` supplies the read target. Multiple same-name owners and ordinary literal support are normal
evidence, not failures. Source-loading failures remain in `scope.unresolved`;
`failures` is reserved for an otherwise loaded document that cannot be queried.
If no readable initial source remains, execution fails rather than manufacturing
a successful empty scope.

Each document group in a scope search likewise carries its own optional
`sourceContext`; any returned authored span requires it. TLDR-only search is
valid and reports canonical Markdown ranges without manufacturing a logical
root or authored source. An explanation report for tldr-only content may omit
`sourceContext` when it has no authored spans. Span keys resolve only against
their document record; equal numeric keys in two documents never identify the
same source.

One global result offset, limit and payload-copy budget apply after ordering
by evidence class, document BFS position and original IR position. Ordinals
are zero-based in that sequence, not document-local/source-only ordinals.
Thus a later document's direct entry precedes an earlier document's mention,
even with limit 1. Only the aggregate `nextOffset` continues the scope. Candidate/relationship limits apply per document; truncation flags
are combined. An empty page does not change a nonempty aggregate outcome.

Example scope request (response shape is generated by `--schema scope-query`):

```json
{
  "schema": "mant.scope-request/v0.12",
  "scope": {
    "documents": [{ "selector": "git" }],
    "traversal": { "followLinks": false }
  },
  "view": { "kind": "explain", "entry": "--help", "options": { "limit": 20, "offset": 0, "contentBytes": 1048576 } }
}
```

The CLI constructs the same contract with repeated `--document`, or with one positional selector plus `--follow-links`. An interactive scope has no serialized `full` view: the host resolves `DocumentScope` directly, opens its first readable root, and gives the loaded set to the TUI's confirmed text search.

## Full Query Contract

`view.kind = "full"` returns a `QueryBundle`:

| Field | Required | Meaning |
| --- | --- | --- |
| `schema` | Yes | `mant.query/v0.12` |
| `label` | Yes | Human-readable source label |
| `address` | No | Exact registered Markdown source or manual name and section |
| `document` | No | Normalized `mant.document/v0.12` document |
| `tldr` | No | Normalized external or embedded quick reference |

A successful runtime result contains useful `document`, `tldr`, or both. The
CLI permits a tldr-only result only for an explicit `--tldr` invocation; an
ordinary document query remains a failed lookup and receives a command hint.
Recoverable parser and IR findings are nested under
`document.diagnostics`; they are not duplicated as a top-level `QueryBundle`
field. Focused outline and excerpt projections instead copy their relevant
findings into their own `diagnostics` field.
For that explicit lookup, personal embedded quick references precede positive
source priorities, cached tldr at the built-in priority-zero baseline, and
non-positive sources. A matching Markdown document without embedded tldr is
skipped rather than blocking a lower quick-reference candidate. A Markdown file
can provide an embedded quick reference and a document in the same bundle.

The embedded form is an input-layer extension, not an additional wire shape.
At the physical start of a Markdown source, invisible
`<!-- mant:tldr:start -->` and `<!-- mant:tldr:end -->` comment lines delimit a
tldr-pages-format preface. ManT emits it as the same `TldrDocument` used for
cached pages, so this syntax does not add another schema or response variant.

An abbreviated but structurally valid Markdown result is:

```json
{
  "schema": "mant.query/v0.12",
  "label": "guide.md",
  "document": {
    "schema": "mant.document/v0.12",
    "producer": {
      "name": "mant",
      "version": "0.12.0",
      "engine": {
        "name": "pulldown-cmark",
        "version": "0.13"
      }
    },
    "sources": [{
      "key": 1,
      "identity": {"kind": "path", "name": "guide.md"},
      "format": "markdown",
      "decodedByteLength": 0,
      "coordinates": {"kind": "decoded-utf8-bytes"}
    }],
    "rootSource": 1,
    "meta": {
      "title": "Guide"
    },
    "sections": []
  }
}
```

The actual `producer.version` is the installed ManT version; clients must not
hard-code the illustrative value above.

## Document Response and IR Projection

`DocumentResponse` is the v0.12 wire projection of ManT's renderer-neutral
`mant-ir::Document`. It describes semantics and normalized layout without
exposing libmandoc pointers, roff macro nodes, internal indexes, HTML, or TUI
components. Schema and producer metadata belong to the response envelope, not
to the reusable in-memory IR.

### Document Envelope

| Field | Meaning |
| --- | --- |
| `schema` | Exact `mant.document/v0.12` marker |
| `producer` | ManT version and parser engine |
| `sources`, `rootSource` | Dense source table and the key of the document root source |
| `contentStore` | Authoritative owner/root/atom/point/link records referenced by the document tree |
| `meta` | Normalized title, section, date, volume, OS, architecture, names, and alias target |
| `fragmentAliases` | Optional exact fragments resolving to `document-overview` |
| `diagnostics` | Optional recoverable parser findings |
| `blocks` | Optional content before the first addressable section |
| `sections` | Recursive section tree |

`producer.engine` is `libmandoc` for man/mdoc input and `pulldown-cmark` for
Markdown.

Each source `format` is one of `man`, `mdoc`, or `markdown`. Temporary
decompression paths never replace the caller-visible path identity.
`SourceKey` values are nonzero, dense, one-based table positions; `rootSource`
is key 1. Every `SourceSpan.source` must select a record in the same table, and
exact byte ranges must fit that record's `decodedByteLength`. Real decoding
rejects empty/non-dense tables, invalid roots, unknown keys, out-of-range spans,
and the former singular `source` object.

Diagnostic levels are `style`, `warning`, `error`, and `unsupported`.
A diagnostic can include a stable code and an original `SourceSpan`.
Recoverable diagnostics do not imply that the returned document is unusable.
Every diagnostic requires `impact`: `none` or `semantic-coverage`. The latter
marks rejected or incomplete semantic coverage independently of severity and
producer-specific codes. Missing or unknown impacts are rejected, including
when a third-party producer supplies the document.

### Sections and Source Locations

Each section contains:

- a document-local `id`;
- optional exact `fragmentAliases` resolving to that `id`;
- a visible `title`;
- optional `spacingBeforeLines`;
- semantic `blocks`;
- recursive `children`;
- an optional original `source` span.

`SourceSpan.line` and `SourceSpan.column` are one-based positions in the
original source. `endLine` and `endColumn` are optional because not every
parser node exposes an exact end location. When available, `byteRange` is the
canonical machine-facing half-open UTF-8 range with zero-based `start` and
`end` offsets. Markdown supplies it exactly; native roff nodes may omit it.

Section depth comes from the tree, not a stored heading-level integer.

Document and section headings carry authoritative `heading.content` inlines rather than a plain section `title`. Optional `Document.heading` preserves an extracted Markdown H1; native bibliographic titles remain in `meta.title`. Outline/excerpt `displayTitle` is a derived plain label, not a second IR fact. A document-root excerpt includes its optional heading and root blocks, so a title-only document remains readable. The unreleased v0.12 shape rejects obsolete section `title` fields instead of silently dropping links. Inline text and destinations resolve through `contentStore`, not through independently copied node values.
Section and explicit anchor IDs share one namespace within a document.

### Block Variants

Every block is tagged by `type`:

| `type` | Principal fields | Meaning |
| --- | --- | --- |
| `paragraph` | `children` | Filled prose |
| `preformatted` | `children`, optional `language` | Literal/code display |
| `list` | structured `kind`, `items`, `compact` | Bullet, ordered, or plain list |
| `definition-list` | `items`, `compact` | Terms with block-capable descriptions |
| `table` | `rows` | Block-capable cells, spans, and alignment |
| `equation` | `value`, `display` | Preserved equation source |
| `vertical-space` | `lines` | Explicit source-requested blank rows |
| `thematic-break` | None | Semantic horizontal break |
| `unsupported` | optional `name`, `text` | Visible source ManT could not structure |

In native `v0.12`, every table row has a closed `kind`. `data` is the default
and is omitted when serialized; a missing field therefore remains compatible
with older data rows. `horizontal-rule` and `double-horizontal-rule` are
whole-row rules and must contain zero data cells. A `layout-rule` is encoded as
`{"layout-rule":{"cells":[...]}}`; its nonempty `cells` list contains
`horizontal` or `double-horizontal` for each logical column and preserves mixed
tbl `_`/`=` layouts. Unknown kinds are rejected. Any rule row carrying ordinary
table cells fails IR validation with `ir.invalid-table-rule-cells`.
Within a `data` row, a cell's omitted `kind` is `text`. The other closed values
are `horizontal-rule`, `double-horizontal-rule`, `isolated-horizontal-rule`,
and `isolated-double-horizontal-rule`. They preserve a rule in one logical
column while adjacent cells remain ordinary data; rule cells carry no ordinary
`blocks`, and violations fail validation with `ir.invalid-table-rule-content`.

Most content blocks may also carry:

- `layout.indentColumns`;
- `layout.spacingBeforeLines`;
- an original `source` span.

An omitted layout is equivalent to zero indentation and zero leading rows.
Renderers should consume these normalized hints rather than reconstruct roff
spacing.

List item `blocks` can contain nested lists and displays. List `kind` is
`{"kind":"bullet"}`, `{"kind":"plain"}`, or `{"kind":"ordered","start":3}`.
Only the ordered variant accepts optional u64 `start`. Missing/null means
unknown, is omitted canonically, and displays from one. Zero and u64::MAX are
valid; excerpt numbering saturates rather than wrapping. Old string kinds,
block-level `start`, and bullet/plain `start:null` are rejected.
Table cells contain `blocks`;
`columnSpan` and `rowSpan` default to `1`, and `alignment` can be `left`,
`center`, or `right`.

Both ordinary-list and definition-list items may carry the same `entry` facts.
Their `forms` and `nameBindings` reference final-IR inline positions rather than
duplicating or removing the visible head. See [mant-ir(7)](mant-ir.md) for
binding and relationship validation. Both are authoritative content owners,
not behavioral equivalence claims.

### Semantic Definitions

A definition item contains rendered `terms`, block-capable `description`,
optional `layout`, optional original `source`, and optional `entry` facts.
An ordinary list item contains `blocks`, optional original `source`, and
optional `entry` facts. Missing/null `entry` and item `source` mean absent and
are omitted canonically.

Facts use structured `kind`: `{"kind":"parameter","parameterKind":"option"}`
(also `marker` or `operand`), or `{"kind":"command"}` (also
`configuration-key`, `environment-variable`, `variable`, `value`, `term`).
`case` is `sensitive` or ASCII `insensitive`, without changing executable spelling.
`names` is the validated selectable-name collection; it does not imply that
the names are equivalent. Only explicit `aliasGroups` and `aliasOf` record
those relationships. Consumers must not rebuild visible content from names.

Definition-owner fragment (the enclosing document must supply the referenced
`contentStore`):

```json
{
  "type": "definition-list",
  "items": [
    {
      "terms": [[{"type":"code","content":{"atom":1,"bytes":{"start":0,"end":17}}}]],
      "description": [{"type":"paragraph","children":[{"type":"text","content":{"atom":2,"bytes":{"start":0,"end":20}}}]}],
      "layout": {"inlineTerm":true,"spacingBeforeLines":0},
      "entry": {
        "id":"option-exclude",
        "kind":{"kind":"parameter","parameterKind":"option"},
        "case":"sensitive",
        "names":["--exclude"],
        "forms":[{"parts":[{"root":{"kind":"term","index":0},"path":[]}]}],
        "nameBindings":[{"name":0,"evidence":"lexical","occurrences":[{"parts":[{"root":{"kind":"term","index":0},"path":[0],"bytes":{"start":0,"end":9}}]}]}]
      }
    }
  ]
}
```

Ordinary-list-owner fragment (with its own enclosing `contentStore`):

```json
{
  "type":"list",
  "kind":{"kind":"ordered","start":3},
  "items":[{
    "blocks":[{"type":"paragraph","children":[{"type":"code","content":{"atom":1,"bytes":{"start":0,"end":17}}},{"type":"text","content":{"atom":2,"bytes":{"start":0,"end":22}}}]}],
    "entry":{
      "id":"option-exclude",
      "kind":{"kind":"parameter","parameterKind":"option"},
      "case":"sensitive",
      "names":["--exclude"],
      "forms":[{"parts":[{"root":{"kind":"block","index":0},"path":[0]}]}],
      "nameBindings":[{"name":0,"evidence":"declared","occurrences":[{"parts":[{"root":{"kind":"block","index":0},"path":[0],"bytes":{"start":0,"end":9}}]}]}]
    }
  }]
}
```

Missing/empty facts `forms` means unrecorded; null forms are invalid shape.
Shape-valid but out-of-bounds references stay in the tree with semantic
validation diagnostics. They never transfer a child's content to its parent.

Definition `layout.inlineTerm` indicates that the term and first description
line fit the same row. Missing/empty layout uses the default; null layout is
rejected. `layout.spacingBeforeLines` missing/null inherits list compactness;
explicit zero is retained. The item-level hints are distinct from block-level
indentation and spacing.

Rejected pre-convergence shapes: item `identity`, facts' flat `role` or semantic
`aliases`, and item-level `inlineTerm` / `spacingBeforeLines`. They are rejected
even when mixed with valid fields. These carriers reject unknown and duplicate
fields in actual decoding, not only in the generated schema.

### Inline Variants

Inline nodes are tagged by `type`:

| `type` | Fields | Consumer behavior |
| --- | --- | --- |
| `text` | `content` | Resolve a checked UTF-8 slice of one logical atom |
| `strong` | `children` | Strong emphasis |
| `emphasis` | `children` | Emphasis |
| `code` | `content` | Resolve a checked literal slice of one logical atom |
| `link` | `occurrence`, `children` | Resolve one typed logical link occurrence |
| `anchor` | `point`, `id`, optional `fragmentAliases` | Resolve a zero-width point and normalized destination |
| `line-break` | `atom` | Resolve one logical hard-break atom |

`content` has an `atom` key and half-open UTF-8 `bytes` within that atom's
logical text. Keys are nonzero, dense, one-based, and valid only inside their
own `ContentStore`; equal numbers in different responses do not identify the
same content. The store has ordered `owners`, `roots`, `atoms`, `points`, and
`links`. An owner groups structural content; a root is one independent logical
sequence. Atoms retain text or whitespace, hard breaks, zero-width break
opportunities, style, optional semantic role/link membership, and authored or
generated provenance. A display override can change a glyph but cannot change
the authoritative logical text. Visual wraps are not new atoms.

A `link.occurrence` selects one store `links` record containing the typed
`target`, optional non-visible `title`, complete ordered logical `label`, and
destination provenance. Several visible link wrappers can share that one
occurrence across a style or structural boundary. A link label can include
content slices and hard-break atoms, including parts in different roots; a
renderer must not infer the destination from the displayed children. The
anchor `point` selects an exact zero-width boundary in a root, independently
of the public normalized `id` and any exact fragment aliases.

Every resolved link occurrence's `target` is tagged by `kind`: `external { uri }`,
`email { address }`, `document { name, fragment? }`,
`manual { name, manualSection? }`, or `section { id }`. Visible child content must
be preserved even when a consumer cannot activate a link. A section target is
a document-local ID, not a generated Markdown slug. A document target retains
the extension-free relative path derived from a `.md` or `.markdown` link and
is resolved lexically only inside the current registered source; `..` cannot
cross that source boundary. Manual targets come directly from mdoc `Xr` and
GNU man `MR`, or conservatively from an unambiguous strongly styled
`name(section)` pair in a traditional man page.

These target kinds have different authority. `section` stays inside the
current document; `document` and `manual` are the only edges followed by a
bounded `DocumentScope`; `external` and `email` are explicit host actions and
never expand a query. The protocol projects the typed IR intent and does not
turn an unresolved target, physical path, or rendered label into another kind.

Document-root, section, and anchor identities are normalized document-local
destinations. `fragmentAliases`, when present, are exact source-authored
fragment spellings that resolve to that identity. They are not node selectors
and need not follow the lower-case `NodeId` grammar. Consumers preserving
anchors emit both the canonical ID and its aliases; lookup must reject an alias
that maps to more than one target.

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
| `sourcePath` | Optional source provenance in diagnostic-oriented CLI JSON; omitted from MCP |
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
| `schema` | `mant.outline/v0.12` |
| `entries` | Echoed `none`, `summary`, `all`, or role-filtered projection |
| `root` | Optional selector used as the returned tree root |
| `label` | Query label |
| `displayTitle` | Optional derived visible title; authoritative heading content remains in the IR |
| `address` | Exact logical address, omitted for direct-file input |
| `sourceContext`, `meta` | Optional dense source table/root key and document metadata |
| `diagnostics` | Optional recoverable parser findings |
| `semanticsComplete` | Present as `false` when semantic declarations were rejected, native definitions could not be classified without guessing, or shared IR validation found an identity or relationship violation |
| `nodes` | Recursive addressable tree |
| `references` | Independent bounded occurrence inventory, policy, counts, source locations and resolution stages |

Node kinds are:

| `kind` | Path convention | Additional fields |
| --- | --- | --- |
| `tldr` | `0` | Reserved quick reference |
| `document-root` | `root` | Optional document heading and content before the first section; a heading-only root is readable; optional `entrySummary` |
| `document-section` | `1`, `1.2`, `1.2.1` | Recursive `children`; optional `entrySummary` |
| `document-entry` | `1.2/e3`, `1.2/e3/e2` | Exact original `owner` coordinates, `entryKind`, selectable `names`, forms, explicit `aliasGroups` / `aliasOf`, optional `documentTargets`, value domain and nested children |

The default `summary` projection emits no individual entries. Instead, each
non-empty root or section scope carries recursive counts for direct entries,
descendants, authored forms, and semantic kinds. `all` emits every entry;
`kinds` emits selected kinds and the ancestors required to preserve their
hierarchy. Empty summaries are omitted. A missing `semanticsComplete` field means
`true`; the exceptional `false` value distinguishes a genuinely empty index
from one made incomplete by rejected source declarations.

This signal is derived from explicit diagnostic `impact`, not a list of parser
codes. It is a validation signal, not proof that every
documented name was discovered, every relationship is known or every behavior
was modeled correctly. It is distinct from result limits and source coverage;
an omitted or true value does not establish complete semantic recall.

Ordinary list items and native definitions remain the content owners. The semantic index is a rebuildable
projection that groups these owners into content records. `names` are exact
explanation names, not read selectors, while `forms` preserve complete authored syntax such as several
accepted `ssh -L` argument layouts or `[+-]O [shopt_option]`. `entryKind`
distinguishes commands; option, marker, and operand parameters; configuration
keys; environment variables; variables; values; and unclassified terms.
Sharing one record does not prove that its names are behaviorally equivalent
or that its forms completely specify a command grammar.
Nested values can declare a `choices` value domain. Cross-document `entry-set`
domains are available only when a producer has explicit evidence; ManT does not infer them from prose. An entry-set carries the authored `reference`, an exact `address` when namespace-only resolution is possible, and the accepted `entryKinds`. An unqualified manual reference or direct-file input can therefore retain its source reference without claiming an exact address.

`documentTargets` has the same reference/address distinction but serves
navigation rather than value validation. Each target records the visible
linked-term `label`. Plain-text outlines render a differing label as
`label → logical-target`, retain an authored `#fragment` after a resolved
Markdown address, and list multiple aliases even when they share one target.
The entry's own `id`, not a separate target array, selects its authoritative
content owner.

This is a projection boundary rather than a second semantic model. The
content-attached `EntryFacts`, derived `SemanticEntry`, and selected
outline node have distinct responsibilities described in
[mant-ir(7)](mant-ir.md). Clients use the versioned fields shown here; they
must not serialize an in-process IR type as a substitute for this contract.

The content shape is specified once under Semantic Definitions above. Full
outlines and compact trail references use `entryKind` for its structured category;
explanation entry metadata uses `kind`. Every selectable-name collection is
`names`; explicit `aliasGroups`, `aliasOf`, and exact `fragmentAliases` retain
their separate meanings.

Environment-variable aliases share one source-neutral grammar across native
and Markdown documents: bare `NAME`, shell `$NAME`, PowerShell `$Env:NAME` or
`${Env:NAME}`, Windows `%NAME%`, and one assignment `NAME=value`. Assignment
values are excluded from semantic names but retained in `forms`. These are
explanation facts, not structural read-selector conveniences. The complete
definition term must match, apart from one explicitly delimited trailing
parenthetical annotation such as Readline's `(On)` default notation. ManT never
takes only its first word, promotes a shell example label, or scans prose. A definition-shaped term that cannot be
classified in a native semantic section remains a generic term, emits
`manual.semantic-entry.unclassified-definition`, and makes `semanticsComplete`
false. Composite headings such as `ENVIRONMENT OPTIONS` use the more specific
option grammar.

Paths are convenient snapshot-local locations. Canonical IDs can survive
unrelated section numbering changes, but are not guaranteed across edits.
Neither is globally unique across
documents. Entry paths such as `28.4/e29` are source-order coordinates rather
than stable IDs and can move when the source manual changes. The separately
returned `id` is document-local; inferred native IDs use a role-qualified full
semantic name and do not reuse shorter formatter navigation tags. Colliding
inferred identities use deterministic semantic fingerprints rather than
source-order suffixes; unrelated section insertion and sibling reordering do
not redirect them. Native section IDs similarly count only repeated equal
headings. IDs still do not promise compatibility after the logical identity or
underlying host manual changes, so clients should rediscover and reuse values
from a current response.

An illustrative response is:

```json
{
  "schema": "mant.outline/v0.12",
  "references": {
    "policy": {"mode": "none", "targetTypes": ["document", "manual"], "offset": 0, "limit": 100},
    "coverage": {"steps": 0, "bytes": 0, "status": {"kind": "not-scanned"}},
    "targetCoverage": null,
    "occurrences": {"kind": "unknown", "reason": "disabled"},
    "targets": {"kind": "unknown", "reason": "disabled"},
    "page": {"offset": 0, "returned": 0, "limited": null, "nextOffset": null},
    "records": []
  },
  "entries": {"kind": "all"},
  "label": "tool.md",
  "address": {"kind": "markdown", "path": "tool", "origin": {"kind": "documents"}},
  "sourceContext": {
    "sources": [{"key": 1, "identity": {"kind": "path", "name": "tool.md"}, "format": "markdown", "decodedByteLength": 0, "coordinates": {"kind": "decoded-utf8-bytes"}}],
    "rootSource": 1
  },
  "meta": {},
  "displayTitle": "Tool",
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
      "entrySummary": {
        "direct": 1,
        "descendants": 0,
        "forms": 1,
        "byKind": [
          {
            "kind": {"kind": "parameter", "parameterKind": "option"},
            "count": 1
          }
        ]
      },
      "children": [
        {
          "kind": "document-entry",
          "path": "1/e1",
          "id": "option-help",
          "owner": {"kind": "owner", "sections": [0], "blocks": [{"kind": "block", "index": 0}], "itemIndex": 0},
          "title": "-h, --help",
          "entryKind": {"kind": "parameter", "parameterKind": "option"},
          "case": "sensitive",
          "names": [
            "-h",
            "--help"
          ],
          "forms": ["-h, --help"]
        }
      ]
    }
  ]
}
```

### Independent Reference Inventory

An outline's `references` inventory is separate from its readable `nodes`.
Only actual logical `LinkOccurrenceKey` values referenced by original
`Inline::Link` nodes count: headings, paragraphs, unannotated items,
definition terms, displays and table cells all participate. Multiple wrappers
for one logical link count once; independently authored repeated links retain
distinct keys. Derived entry forms or `documentTargets` never add occurrences.
Distinct-target counting includes target type
and fragment. `ValueDomain::EntrySet` is a separate scope relationship, not a
visible link. Neither inventory nor semantic annotations rewrite body content.

The request policy is `{ "mode": "summary", "targetTypes": ["document", "manual"], "offset": 0, "limit": 100 }`.
Modes are `none` (no reference traversal), `summary` (counts only), and `all`
(counts plus a bounded occurrence page). Other types are `local`, `external`
and `email`. Root selection limits the original source subtree; entry-kind
filters do not remove its references. `limit` is 1–1000. Offset skips selected
occurrences, not distinct targets, and consumes work on each stateless call.

Counts are tagged `exact {value}`, `lower-bound {value}`, or `unknown {reason}`.
`coverage` reports charged steps/bytes and complete, limited, or not-scanned
status. `targets` may be a lower bound while `occurrences` is exact if the
bounded deduplication set filled. `page.limited` describes retained output,
not source coverage. `nextOffset` is supplied only when progress to a further
occurrence is established; a scan limit does not promise a reachable next page.

The response-local `contentProjection` is a closed resolver store, not the
filtered reference page: retaining one link may also retain unselected atoms
and links in its logical root. Only `records` participate in page and target
counts or the requested `targetTypes` filter. Consumers must not enumerate
`contentProjection.contentStore.links` as additional search results.

The default scan permits 250,000 work units, depth 256 and 8 MiB of inspected
text. In-process limits cannot exceed 1,000,000 units or 32 MiB. Distinct-target
retention stops at 4,096 keys or 1 MiB of key text. Labels retain at most 4 KiB
of UTF-8; positions at most 8 KiB. Returned record retention defaults to
256 KiB and cannot exceed 1 MiB. These bounds apply before copying or indexing,
including optional form associations and local-target validation. Summary
does not collect all labels/positions or build a cloned semantic-form index.

Each record has a response-local `occurrence`, `origin` (checked final-IR
`ContentLocation`), optional original `owner`, bounded `labelPreview` and
`labelPreviewTruncated`, atomic form `association`, and staged `resolution`.
The complete typed target and label are resolved through `contentProjection`;
the preview is not a second content authority. `sourceRead` is an exact selector for
the containing readable local subtree, not an occurrence selector or remote
read. Source positions are not byte offsets, text matches, terminal cells or
cross-call revision tokens. They remain stable across filtering, pagination,
wrapping and serialization of the same IR, not across source edits.

Resolution never loads another document during ordinary outline projection.
`logical-address` only proves namespace resolution; its fragment is absent or
unchecked. `not-queried` retains unresolved manual intent, including a missing
manual section; `missing-context` means a direct file has no registered source
namespace. Only the already loaded source can report `loaded` with a fragment
that is absent, valid, missing, ambiguous or limited. `targetCoverage` records
the optional local validation scan, sharing the same operation budget.
External/email targets are not probed. Missing/ambiguous destinations do not
become successful reads. To explore another document in MCP, explicitly use
its logical catalog address in a subsequent find/outline/read operation.

Scope traversal reuses this bounded content walk, merges independent EntrySet
relations in source order, and deduplicates document addresses separately.
`scope.referenceLimits` lists loaded documents whose outbound scan could not
finish; it is distinct from known depth/document/content `frontier` edges.
An incomplete scan cannot establish absence of further links.

#### Associated navigation

Heading origins and nonempty validated form associations can add an inline
reference badge to a visible content node. `document-entry.owner` is the exact
original item coordinate; `association.owner` identifies the semantic owner
whose forms were checked, which can differ from the nearest content `owner`
through an unannotated nested item. Clients must compare these structural
coordinates, not labels, IDs or target names. Invalid/limited bindings never
authorize a form badge. Hidden or ambiguous owners retain ordinary references.

Plain and ANSI outline output show these badges only from returned `all`
records; `summary` remains a global bounded count, and `none` does not scan.
The independent inventory retains every occurrence, including a form link and
a body link with the same target. A badge may group exact typed targets,
including fragments, but a partial page only describes known targets and must
not claim uniqueness. A badge never opens a target or creates a read selector.
Reference/link styling is independent from heading and semantic-entry styling.

Reference inventory input:

```markdown
# Linked tool

## Commands

<!-- mant:entries role=command case=sensitive -->
- [`tool`](other.md#part): See [details](other.md#part).
```

Complete reference inventory example (registered as `documents/linked`):

```json
{
  "schema": "mant.outline/v0.12",
  "entries": {
    "kind": "all"
  },
  "references": {
    "contentProjection": {
      "contentStore": {
        "owners": [
          {
            "key": 1,
            "kind": "content",
            "roots": [
              1
            ],
            "provenance": {
              "kind": "unknown"
            }
          }
        ],
        "roots": [
          {
            "key": 1,
            "owner": 1,
            "kind": "body",
            "atoms": [
              1,
              2,
              3,
              4
            ],
            "points": [],
            "provenance": {
              "kind": "unknown"
            }
          }
        ],
        "atoms": [
          {
            "key": 1,
            "root": 1,
            "owner": 1,
            "kind": "text",
            "text": "tool",
            "style": {
              "literal": true
            },
            "link": 1,
            "provenance": {
              "kind": "authored",
              "span": {
                "source": 1,
                "byteRange": {
                  "start": 81,
                  "end": 87
                },
                "line": 6,
                "column": 4,
                "endLine": 6,
                "endColumn": 10
              }
            }
          },
          {
            "key": 2,
            "root": 1,
            "owner": 1,
            "kind": "text",
            "text": ": See ",
            "provenance": {
              "kind": "authored",
              "span": {
                "source": 1,
                "byteRange": {
                  "start": 103,
                  "end": 109
                },
                "line": 6,
                "column": 26,
                "endLine": 6,
                "endColumn": 32
              }
            }
          },
          {
            "key": 3,
            "root": 1,
            "owner": 1,
            "kind": "text",
            "text": "details",
            "link": 2,
            "provenance": {
              "kind": "authored",
              "span": {
                "source": 1,
                "byteRange": {
                  "start": 110,
                  "end": 117
                },
                "line": 6,
                "column": 33,
                "endLine": 6,
                "endColumn": 40
              }
            }
          },
          {
            "key": 4,
            "root": 1,
            "owner": 1,
            "kind": "text",
            "text": ".",
            "provenance": {
              "kind": "authored",
              "span": {
                "source": 1,
                "byteRange": {
                  "start": 133,
                  "end": 134
                },
                "line": 6,
                "column": 56,
                "endLine": 6,
                "endColumn": 57
              }
            }
          }
        ],
        "points": [],
        "links": [
          {
            "key": 1,
            "owner": 1,
            "target": {
              "kind": "document",
              "name": "other",
              "fragment": "part"
            },
            "label": [
              {
                "kind": "content",
                "content": {
                  "atom": 1,
                  "bytes": {
                    "start": 0,
                    "end": 4
                  }
                }
              }
            ],
            "provenance": {
              "kind": "authored",
              "span": {
                "source": 1,
                "byteRange": {
                  "start": 80,
                  "end": 103
                },
                "line": 6,
                "column": 3,
                "endLine": 6,
                "endColumn": 26
              }
            }
          },
          {
            "key": 2,
            "owner": 1,
            "target": {
              "kind": "document",
              "name": "other",
              "fragment": "part"
            },
            "label": [
              {
                "kind": "content",
                "content": {
                  "atom": 3,
                  "bytes": {
                    "start": 0,
                    "end": 7
                  }
                }
              }
            ],
            "provenance": {
              "kind": "authored",
              "span": {
                "source": 1,
                "byteRange": {
                  "start": 109,
                  "end": 133
                },
                "line": 6,
                "column": 32,
                "endLine": 6,
                "endColumn": 56
              }
            }
          }
        ]
      }
    },
    "policy": {
      "mode": "all",
      "targetTypes": [
        "document",
        "manual"
      ],
      "offset": 0,
      "limit": 100
    },
    "coverage": {
      "steps": 133,
      "bytes": 425,
      "status": {
        "kind": "complete"
      }
    },
    "targetCoverage": null,
    "occurrences": {
      "kind": "exact",
      "value": 2
    },
    "targets": {
      "kind": "exact",
      "value": 1
    },
    "page": {
      "offset": 0,
      "returned": 2,
      "limited": null,
      "nextOffset": null
    },
    "records": [
      {
        "occurrence": 1,
        "origin": {
          "kind": "content",
          "sections": [
            0
          ],
          "blocks": [
            {
              "kind": "block",
              "index": 0
            },
            {
              "kind": "list-item",
              "index": 0
            },
            {
              "kind": "block",
              "index": 0
            }
          ],
          "root": {
            "kind": "inlines"
          },
          "path": [
            0
          ]
        },
        "sourceRead": {
          "kind": "path",
          "path": "1"
        },
        "owner": {
          "kind": "owner",
          "sections": [
            0
          ],
          "blocks": [
            {
              "kind": "block",
              "index": 0
            }
          ],
          "itemIndex": 0
        },
        "labelPreview": "tool",
        "labelPreviewTruncated": false,
        "association": {
          "kind": "valid",
          "owner": {
            "kind": "owner",
            "sections": [
              0
            ],
            "blocks": [
              {
                "kind": "block",
                "index": 0
              }
            ],
            "itemIndex": 0
          },
          "forms": [
            0
          ]
        },
        "resolution": {
          "kind": "logical-address",
          "address": {
            "kind": "markdown",
            "path": "other",
            "origin": {
              "kind": "documents"
            }
          },
          "fragment": {
            "kind": "unchecked"
          }
        }
      },
      {
        "occurrence": 2,
        "origin": {
          "kind": "content",
          "sections": [
            0
          ],
          "blocks": [
            {
              "kind": "block",
              "index": 0
            },
            {
              "kind": "list-item",
              "index": 0
            },
            {
              "kind": "block",
              "index": 0
            }
          ],
          "root": {
            "kind": "inlines"
          },
          "path": [
            2
          ]
        },
        "sourceRead": {
          "kind": "path",
          "path": "1"
        },
        "owner": {
          "kind": "owner",
          "sections": [
            0
          ],
          "blocks": [
            {
              "kind": "block",
              "index": 0
            }
          ],
          "itemIndex": 0
        },
        "labelPreview": "details",
        "labelPreviewTruncated": false,
        "association": {
          "kind": "valid",
          "owner": {
            "kind": "owner",
            "sections": [
              0
            ],
            "blocks": [
              {
                "kind": "block",
                "index": 0
              }
            ],
            "itemIndex": 0
          },
          "forms": []
        },
        "resolution": {
          "kind": "logical-address",
          "address": {
            "kind": "markdown",
            "path": "other",
            "origin": {
              "kind": "documents"
            }
          },
          "fragment": {
            "kind": "unchecked"
          }
        }
      }
    ]
  },
  "label": "linked.md",
  "displayTitle": "Linked tool",
  "address": {
    "kind": "markdown",
    "path": "linked",
    "origin": {
      "kind": "documents"
    }
  },
  "sourceContext": {
    "sources": [
      {
        "key": 1,
        "identity": {
          "kind": "path",
          "name": "linked.md"
        },
        "format": "markdown",
        "decodedByteLength": 135,
        "coordinates": {
          "kind": "decoded-utf8-bytes"
        }
      }
    ],
    "rootSource": 1
  },
  "meta": {},
  "nodes": [
    {
      "kind": "document-root",
      "path": "root",
      "id": "document-overview",
      "title": "OVERVIEW"
    },
    {
      "kind": "document-section",
      "path": "1",
      "id": "commands",
      "title": "Commands",
      "entrySummary": {
        "direct": 1,
        "descendants": 0,
        "forms": 1,
        "byKind": [
          {
            "kind": {
              "kind": "command"
            },
            "count": 1
          }
        ]
      },
      "children": [
        {
          "kind": "document-entry",
          "owner": {
            "kind": "owner",
            "sections": [
              0
            ],
            "blocks": [
              {
                "kind": "block",
                "index": 0
              }
            ],
            "itemIndex": 0
          },
          "path": "1/e1",
          "id": "command-tool",
          "title": "tool",
          "entryKind": {
            "kind": "command"
          },
          "case": "sensitive",
          "names": [
            "tool"
          ],
          "forms": [
            "tool"
          ],
          "documentTargets": [
            {
              "label": "tool",
              "reference": {
                "kind": "document",
                "name": "other",
                "fragment": "part"
              },
              "address": {
                "kind": "markdown",
                "path": "other",
                "origin": {
                  "kind": "documents"
                }
              }
            }
          ]
        }
      ]
    }
  ]
}
```

This response is checked against the actual Markdown producer and projection.
The two source locations resolve independently in the original IR; their one
destination is only namespace-resolved, with its fragment still unchecked.

## Excerpt Projection

`mant.excerpt/v0.12` returns complete selected content without returning unrelated
sections:

| Field | Meaning |
| --- | --- |
| `schema` | `mant.excerpt/v0.12` |
| `label` | Query label |
| `address` | Optional logical namespace for references in selected content |
| `semanticsComplete` | Same document-wide completeness signal as outline; omitted when true |
| `producer`, `sourceContext`, `meta` | Optional producer, dense source table/root key, and metadata |
| `diagnostics` | Relevant recoverable findings |
| `contentProjection` | Closed response-local store for retained document selections; absent when none need content |
| `selections` | Selected content in source order |

The projection contains a `contentStore` and remaps every retained inline key
into that response-local store. It is not a second editable document. A key in
an excerpt cannot be used to read the original document or another response.

Selection kinds are:

- `tldr`, containing one complete `TldrDocument`;
- `document-root`, containing an optional authoritative `heading` and root `blocks`; a heading-only result may have no blocks;
- `document-section`, containing a complete section subtree;
- `document-entry`, whose `entry` is a single-item `list` or `definition-list`
  block containing the original owner and its complete descendants. Preserve
  the block's layout and compactness; an ordered list starts at the selected
  item's original ordinal, including when earlier siblings are omitted.

The unreleased v0.12 entry payload is a block, not the former standalone
definition item. Read shared facts from its sole list item's `entry` or native
definition's `entry`. In-process consumers can use `Block::entry_owner()`;
renderers consume the original block instead of converting ordinary items into
term-and-description content.

Selections retain source-neutral IR: an entry-set `valueDomain` carries its
authored reference and source span, not a catalog lookup result. Outline uses
the protocol-owned summary with a namespace-resolved `address` instead.
Excerpt clients can use the excerpt's `address` with
`DocumentReference::resolve_from`, or request the corresponding outline
node for its resolved relationship summary. Direct-file inputs have no logical
namespace; an address never proves that a target document is installed.

The completeness signal also travels inside single- and multi-document
explanations. Text and MCP excerpts retain a concise incomplete-semantics notice
even when ordinary parser diagnostics are hidden. Full raw document responses
carry diagnostics rather than an outline-completeness claim. Search responses
make no semantic-index completeness claim: search examines rendered content,
not just recognized entries. In-process producers must run
`mant_ir::validate_document` and attach its
findings before handing documents to projection APIs; projections reuse those
findings rather than revalidating the whole tree for every selected node.

Every selection has an `outline` trail. Its `ancestors` array contains compact
`path`, `id`, and `title` references from the outermost section to the direct
parent. Its typed terminal `node` contains the selected node's `kind`, `path`,
`id`, and `title`; a `document-entry` node additionally retains `role`, `case`,
and normalized `names`. Empty ancestor arrays are omitted.

An excerpt accepts from 1 through 16 closed path/ID selector objects. The schema, native request boundary, MCP,
and direct in-process projection all enforce the same bound. Overlapping
selections are deduplicated, and source order is preserved. Selecting a section
includes its descendants. The outline trail identifies ancestors without
copying their blocks.

The `excerpt` and `outline.root` views use one exact structural resolver.
An ID cannot fall back to a same-named entry alias, and a failed path cannot
activate a fragment or another document. Duplicate IDs return candidates;
exact paths keep distinct source owners independently readable. Names may
repeat without a navigation-only ambiguity diagnostic: explain returns their
evidence separately. Genuine invalid/duplicate identities, bindings and explicit
alias-relation conflicts remain validation errors. Anchors and reference
occurrences are reveal positions, not synthetic readable content nodes.
All paths and positions apply to the loaded document only. An edit may make a
previous path point somewhere else; this release has no cross-call stale check.

## Explanation Evidence

`mant.explanation/v0.12` is independent of strict selection. CLI `--explain`,
request JSON and MCP `mant_explain` collect the same immutable IR evidence.
They do not call the unique selector resolver or turn ambiguity errors into
results. There is no strict-explain mode.

| Field | Meaning |
| --- | --- |
| `schema`, `producer`, `query` | Contract identity, implementation version and normalized literal/options |
| `label`, `address` | Source label and optional logical document namespace |
| `sourceContext` | Optional dense source table/root key resolving every returned span |
| `contentProjection` | Closed response-local store shared by retained supports and evidence when they contain document inlines |
| `outcome` | `evidence` or `no-evidence`, evaluated before result pagination |
| `total`, `returned`, `nextOffset` | Matching owner count, current page size and optional continuation |
| `truncation` | Separate `candidates`, `relations`, `content` flags; counts are lower bounds when candidate/relation traversal stops |
| `semanticsComplete`, `diagnostics` | Semantic validation/producer coverage, not a promise of exhaustive recall |
| `order` | Always `class-then-source`: class, document BFS position, original IR position |
| `counts` | Fixed `directEntry`, `relatedEntry`, `entryMention`, `contextMention`, each with `total` and `returned`; zero counts remain present |
| `evidence` | One ordered page of independently owned records, classified before copying or paging |

Each record keeps its zero-based `ordinal`, real `outline` trail, optional
`source` span, and `bases`. Ordinary supporting blocks additionally carry an
IR `blockPath`; their containing root/section is not a manufactured entry.
The required `class` is independent of whether copied `entry` metadata is
present. One owner retains multiple `bases` but exactly one class:

| Priority | `class` | Meaning |
| --- | --- | --- |
| 1 | `direct-entry` | Semantic owner with Name, complete Form or exact Identity basis |
| 2 | `related-entry` | No direct match; reached via a validated explicit alias relationship |
| 3 | `entry-mention` | Semantic owner included only through Literal support |
| 4 | `context-mention` | Matched ordinary block without a semantic owner |

This is an evidence-type order, not a confidence score. Generic owners without
names remain entries; details omitted by budget do not change classification.
Counts sum to response total/returned and, in scope, to source report counts.
Independent same-name owners remain distinct, even across documents with the
same NodeId. `AliasGroup` supplements matched names, not a separate owner/class.
`entry` is present only for a real semantic owner and may itself be omitted. `content` contains its
original single-item block, an ordinary supporting block, or a `declaration-member`
reference into the returned document-local `supports` pool. A `shared-entry`
uses `support`, typed `path` steps and `itemIndex` to select a physical owner
already contained in a returned source fragment. It can be omitted
atomically when it exceeds the copy budget (`contentOmitted`); oversized
facts/forms set `detailsOmitted`. Read the returned node to retrieve original
content independently. Metadata and protocol envelope bytes are outside this
payload-copy budget. Copied support metadata, members, bodies and references
are inside that budget.

A `declaration-group` support contains the original `blockPath`, half-open
`group` range, original member outline/source trails and one complete
definition-list `block`. The last member provides the description; earlier
members retain their empty physical descriptions. This is recovered reading
context, not alias evidence or proof that every sentence applies to every
member. It neither adds candidates nor inherits children or value domains.
Groups and pools each have at most 256 members/items. An unavailable context
sets `supportOmitted` and content truncation rather than claiming no explanation.
Only owner records are paginated: even a one-owner page carries its necessary
support, and multiple direct records share one copy. Scoped pools belong to
their source document, never to a global node-ID namespace.

An `owned-entry` stores a single physical owner's original excerpt when it
also contains another selected owner. A `contained-declaration-group` retains
its own original `blockPath`, `group`, members and provider, but refers through
`support` and a typed `path` to a nested list in an owned fragment. References
point directly to `owned-entry` or `declaration-group`, never another reference.
Containment is source-coordinate based, not equality of text or IDs. The same
body is copied and displayed once while all evidence counts and distinct
providers remain observable. An inner-only page carries just its needed source
context, not an unselected ancestor.

An evidence `support` requires `class: direct-entry`, a matching
`declaration-member` content reference and a resolvable owner whose ID and
forms agree with the evidence. It cannot coexist with `supportOmitted` or
`contentOmitted`. `shared-entry` is also direct-only, with no evidence `support`;
it reuses physical content without claiming a recovered reading relationship.
It may coexist with `supportOmitted` when the original owner fits but its whole
reading group does not. Decoding, explicit validation and offline presentation
share these checks; a bare valid pool index cannot authorize unrelated content.

Every record includes `previews` and `previewsOmitted`. Without Literal support
they are `[]` and `false`. Otherwise at most two distinct matched blocks are
represented in original order. Each window is at most 1024 Unicode scalars and
contains one complete match; it has `text`, half-open `matchStartChar` /
`matchEndChar`, `clippedBefore`, `clippedAfter`, `blockPath`, and optional actual
block `source`. Controls are masked before ranges are measured. Markdown/ANSI
rendering cannot change the wire text/ranges. The path starts with `root` or
`sections/sN[/sN...]`, then zero-based `bN` blocks, `iN` list items, `dN`
definition descriptions, or `rN/cN` table cells; it resolves in the exact final
IR using `mant_query::resolve_explanation_block`, not in exported Markdown.
These are snapshot-local locations, not durable NodeIds.

The one `contentBytes` budget first reserves direct-match facts for the page,
then direct bodies and necessary group context, then optional details/windows
and weaker evidence in class/source order. A window that does not fit is omitted whole
and sets `previewsOmitted`, also setting content truncation. Choosing two
representative blocks and clipping context are not budget failures. `content`
never contains a snippet disguised as complete IR. Compact text/Markdown/MCP
show full direct/related bodies and only actual preview windows plus read
coordinates for mentions. A mention in `-Q` is not a definition of `--help`.
Empty categories have no headings, but their summary counts remain; direct
entries outside this page are explicitly distinguished from zero collected
direct entries. Neither case proves that a command lacks the queried option.

Report headings keep coordinates and the original title separate from `Matched by`.
Plain forms/body/preview text has no generated `| ` prefix; authored bars remain.
CommonMark quotes source lines with `> `, including blank lines and fences.
Plain report labels are reading aids, not authenticated source boundaries;
machine consumers use the structured response. Metadata is single-line sanitized and
Markdown-escaped; source text retains its own newlines. Exact available ranges
compose with original inline styles; fenced CommonMark displays remain verbatim.
Plain and ANSI reports use the same layout and class/owner boundaries.
An individual direct/related record suppresses Forms only when its displayed body
already contains the complete matching owner and forms. Mentions and records with
omitted or incomplete bodies retain Forms. An empty independent definition gets
a no-independent-description notice only when no group context is available;
this is distinct from a body/support budget omission. Shared contexts are
displayed once with a numbered reference and the actual description provider.
These presentation choices do not remove facts or forms from the response DTO.

Name bases carry `matches` with actual authored `name` spellings; Form bases
carry `matches` with `sourceFormIndex` and complete `text`. Identity bases carry
`fields` (`id`, `path`), whose values are in `outline`. Empty match details with
`matchDetailsOmitted: true` still mean the basis matched, not no evidence.
`entry.nameBindings` independently projects ordinary names by `nameIndex`;
it never declares an unqueried name to be a match.

Name/Form matches and ordinary bindings carry ordered `occurrences`, each with
a snapshot `sourceOccurrenceIndex` and independent `forms`/`content` arrays.
Form ranges address returned `entry.forms[formIndex]`. Content ranges use
`kind: block-text` or `definition-term`, relative to returned `content.block`.
Typed list-item/definition-item/table-cell steps followed by block steps locate
leaves; definition terms separately specify `itemIndex` and `termIndex`.
All `startChar`/`endChar` ranges are half-open safe-text Unicode scalars before
layout or escaping. A single-item excerpt remaps the original item to index 0.
Preview `contentRanges` supplement the original absolute provenance coordinates.
For `declaration-member`, `support` and `itemIndex` select the returned group
member. Content ranges remain owner-local (outer item zero); use
`ExplanationContent::resolve_range` to map them to the shared body. No reference
targets a body or metadata omitted from the response. `shared-entry` paths
select the containing list before `itemIndex`; content ranges still use the
owner-local outer item zero. Strict response decoding
rejects dangling, wrong-owner and out-of-bounds content/position references.

Per evidence, Name/Form details share a 32-record limit; ordinary name bindings
have a separate 32-record limit. Each record retains at most 32 occurrences,
each occurrence at most 32 fragments per domain; all retained location fragments
including preview mappings share a 1,024-fragment limit. A multi-fragment
occurrence is retained or omitted whole within each domain. `matchDetailsOmitted`
and `nameBindingsOmitted` independently report limits on applicable facts and
locations; both contribute to content truncation. Returned names/forms themselves
are never shortened by these optional binding limits. Self-contained match facts
precede direct bodies/context; optional entry metadata/bindings and previews
follow in the shared copy budget. Body references are committed only after the
whole body is accepted.
For complete field definitions and coordinate examples see the
[explanation architecture](https://github.com/BryanHeBY/ManT/blob/dev/docs/architecture/semantic-explanations.md).

Match bases are `name` (exact documented spelling), `form` (complete authored
form), `identity` (exact entry ID/path), `literal` (ordinary IR text),
`alias-group` (validated local group members), and `related` (starting owner
plus declaration-owner IDs along explicit `aliasOf` edges). Names/forms follow
the owner's declared ASCII case policy. Literal matching is case-sensitive;
name-internal punctuation cannot terminate a match, so `-a` cannot
match `--all`, `-ca` cannot match `-ca.cert`, and `-I` never folds into `-i`.
`-#` does not match `-###`, and `--` does not match `--%`. Sentence-ending
periods/colons may terminate a literal; `--help=CLASS` still mentions `--help`.
Typographic quotes, CJK enclosures and sentence punctuation also delimit prose:
`“--help”` and `--help。` retain literal evidence. This finite separator policy
does not split Unicode letters, combining marks or arbitrary symbols inside
a longer name. There is no shorthand,
NLP, regular expression, executable grammar or inferred synonym relationship.

The same owner's bases are combined. Distinct owners are never merged because
they share names, text or parentage. Valid same-document alias edges can be
followed in either direction for supporting material, preserving declaration
direction and one deterministic path per related owner. This does not inherit
body, children or value domains. Literal-only seeds do not activate relations.
External documents still require the existing authorized scope traversal;
`EntrySet` never becomes local children or implicitly exhaustive choices.

Requests accept at most 512 Unicode scalars and no controls. `options.limit`
is 1–256 (default 50), `offset` is a zero-based unsigned result offset, and
`contentBytes` is 1–4,194,304 (default 1,048,576). Per document, at most 10,000
matching owners and 4,096 relation edges are retained, with chains capped at
32 edges. The bounded pool favors higher classes before original order; later
direct/related records can replace earlier mentions. Any discarded candidate
sets truncation; `nextOffset` reaches only retained candidates, not discarded
ones. Refresh the query after source changes. MCP `startChar` / `maxChars` page the canonical result separately.
Quick-reference-only sources currently have no full-document evidence; this
does not mean their examples contain no useful information.

A valid readable query with one, many or zero evidence records succeeds
(CLI exit 0). Check `outcome`, not whether the page is empty. Partial source
failure preserves readable results and qualified coverage; no readable initial
source is a source error. Invalid input fails before lookup. No query executes
examples, expands real environment values or requests additional authority.

## Search Projection

`mant.search/v0.12` defaults to visible-text search. Flow and TLDR retain the
canonical Markdown visible-text extractor; `scope: "markdown"` searches the
exact user-exportable Markdown bytes, including markup. TLDR remains searchable
without a primary document or invented authored source. A viewport wrap is
never search input. Every scope pages complete occurrences, not rendered lines.

### Result Envelope

| Field | Meaning |
| --- | --- |
| `schema` | `mant.search/v0.12` |
| `label`, `sourceContext`, `meta` | Source identity table/root key and metadata |
| `contentProjection` | Bounded response-local units and fragments closing retained visible matches; absent for Markdown-only or zero-hit pages |
| `query` | Fully normalized search settings |
| `render` | Coordinate-bearing Markdown artifact or Fixed visible-surface descriptor |
| `total` | Exact number of complete occurrences after the full scan |
| `returned` | Number of retained occurrences in this page |
| `offset` | Echoed result pagination offset |
| `truncated` | Whether more hits remain |
| `nextOffset` | Next deterministic offset when truncated |
| `semanticsComplete`, `coverageDetailsOmitted`, `diagnostics` | Coverage state independent of hit count and pagination |
| `matches` | Complete occurrences in snapshot order |

`query` always echoes all defaults, even when the request omitted them.
A no-match search is successful and returns `total = 0` with an empty
`matches` array.

`mant.scope-search/v0.12` uses the same occurrence pagination globally across
the resolved document order. Its `documents` retain only groups with page
hits, while `coverageByDocument` contains every scanned document in scope
order, including zero-hit and paginated-away documents. Each coverage entry
reports `address`, `depth`, `semanticsComplete`, bounded `diagnostics`, and
exact `coverageDetailsOmitted`. A coverage entry includes `sourceContext`
when the scanned document has a primary source; this closes diagnostic spans
even if no hit from that document is retained. A tldr-only entry can omit it
only when no authored span needs a source table. Top-level `semanticsComplete` is their
conjunction; an empty retained page cannot erase a semantic gap. A scan that
cannot account for every document fails rather than returning a false complete
subset. A scoped hit keeps its global ordinal; it has no local pagination
cursor.

In a scope search, coverage diagnostics and source contexts in retained hit
groups share a 32 MiB aggregate serialized-metadata limit. A per-document
diagnostic count bound does not permit 256 independent unbounded copies.

### Authoritative and Display Coordinates

Each `matches` item is one exact occurrence. `location` is a tagged
`visible-flow`, `visible-fixed`, or `markdown-artifact` coordinate. All three
use half-open Unicode scalar ranges (`startScalar`/`endScalar`); visible hits
name a response-local unit, while artifact hits name the exact export and
one-based scalar line/column. `displaySlices` use fragment-relative scalar
ranges and are subordinate presentation fragments, not another authoritative range.
`matchedText` must equal the exact unit substring for visible matches. An
occurrence can span several Flow roots or Fixed runs without being split into
several pagination items. The old `occurrences` line group and nullable
`root`/`logical`/`markdown` coordinates are rejected on decode.

Visible units contain ordered, nonempty UTF-8 fragments and one explicit join
between each adjacent pair. A `render-separator` join carries the exact
separator bytes once; unknown or hard boundaries cannot license a cross-boundary
match. Fragment sources retain explicitly named UTF-8 byte ranges for storage
and source addressing; they are not public text-match positions. They are
tagged Flow content locations, TLDR paths,
render-derived text, or Fixed final row/run slices. The producer checks these
against its input snapshot; the decoder checks the bounded response-local
keys, ranges, UTF-8, joins and `matchedText` without fetching that snapshot.
Count-only scans do not copy the full document into the response. Regex `^`
and `$` follow the searched representation's line boundaries, not a viewport.

For a Flow Markdown-coordinate result, `render` has this shape:

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

Fixed visible results use `schema: "mant.fixed/v1"` and
`format: "fixed-visible"`; terminal cell columns are never used as text
positions. `lineCount` is document-dependent. Artifact `startScalar`/`endScalar`
are half-open Unicode scalar ranges in the exact export; its one-based human
columns also count Unicode scalars. An artifact-only fence hit legitimately has no native
display slice. Regex compilation has a
fixed project resource budget in addition to the pattern-length bound; an
expression whose compiled program exceeds that budget is rejected before
document matching.

Each occurrence includes:

- a one-based global `ordinal` that is not reset by pagination;
- an `outline` trail ending at the nearest reusable node accepted by excerpt
  selection;
- exact `matchedText`, tagged `location`, and zero or more display slices;
- an optional original `nodeSource` span for the owning outline node;
- a human-readable `preview`;
- optional full Markdown context lines.

Text presentations may merge context windows for readability but cannot change
hit counts or offsets. Color decorators preserve visible bytes; the tagged
location, not preview or terminal wrapping, is authoritative.

The trail has the same `ancestors` and typed terminal `node` shape used by
excerpt selections. The node union uses the same `tldr`, `document-root`,
`document-section`, and `document-entry` identities as outlines. Consequently,
search and explain consumers can render one complete tree chain without
reconstructing ancestry from separate fields.

A complete no-match response is:

```json
{
  "schema": "mant.search/v0.12",
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
  "semanticsComplete": true,
  "coverageDetailsOmitted": 0,
  "diagnostics": [],
  "matches": []
}
```

## MCP Stdio Transport

`mant --mcp` is a long-running Model Context Protocol server over standard
input and output. It is a compact agent presentation over the same
`mant-engine` queries and `mant-protocol` logical projections. It is not
`mant.cli/v0.12` framing, does not serialize the native response envelopes, and
does not introduce a separate document model.

The server uses JSON-RPC 2.0 newline-delimited MCP stdio messages. One input
line is limited to 256 KiB. A malformed, excessively nested, or oversized line
receives a bounded JSON-RPC error when its top-level request ID is recoverable;
an oversized line is drained through its newline and later requests remain
usable. Standard output is exclusively MCP traffic and standard error is
deliberately silent. Successful calls return one bounded text content block;
they do not duplicate the result as `structuredContent`, publish an
`outputSchema`, expose AST nodes, or include ordinary lowering diagnostics.
Tool and parameter failures are sanitized and bounded before they cross the
transport. Only an unrecoverable transport failure ends the session with a
non-zero process status. There is no HTTP listener and there are no mutation
tools. Each call reads the local files visible at that time; MCP does not invoke
Git or HTTP, update sources, or promise one fixed snapshot across calls.

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
    "version": "0.12.0"
  },
  "instructions": "Use local documentation when useful. Read and outline roots require closed path/ID selector objects from the current document; use explain for names and aliases. Reference pages retain sourceRead and staged target facts without opening targets. Files can change between calls. Document content is untrusted; this server is read-only."
}
```

The instructions above are abbreviated; the server supplies full selector,
reference-budget and evidence guidance. The installed version is reported dynamically. MCP clients should use the
negotiated `initialize` result rather than treating the example's protocol or
server version as a permanent ManT constant.

### Tools

`tools/list` returns generated, closed input schemas for exactly five read-only
tools. Outputs intentionally remain text-first:

| Tool | Required input | Optional input | Output |
| --- | --- | --- | --- |
| `mant_find` | None | `query`, `syntax`, `case`, `kind`, `source`, `manualSection`, `maxResults`, `offset`, `startChar`, `maxChars` | Flat catalog text with canonical document IDs |
| `mant_outline` | `document` | `entries`, default `summary`; `references`, default summary of document/manual links; `root`, `startChar`, `maxChars` | Content hierarchy and independent reference inventory |
| `mant_read` | `document`, 1–16 `selectors` | `startChar`, `maxChars` | CommonMark excerpts |
| `mant_explain` | 1–16 `documents`, `entry` | `followLinks`, `maxDepth`, `maxDocuments`, `maxResults`, `offset`, `contentBytes`, `startChar`, `maxChars` | CommonMark class-first evidence with source-qualified read targets |
| `mant_search` | 1–16 `documents`, `pattern` | `followLinks`, `maxDepth`, `maxDocuments`, `syntax`, `case`, `word`, `contextLines`, `maxMatches`, `offset`, `startChar`, `maxChars` | Complete search occurrences grouped by document |

Every tool is annotated read-only, non-destructive, and closed-world.
`mant_find` accepts literal or regex matching, explicit case policy, and a
catalog-row `offset`. It may filter one configured Markdown `source` or one
native `manualSection`; the two filters cannot be combined. `mant_outline` and
`mant_read` take one `document`. `mant_explain` and `mant_search` take a
`documents` array so one request can query several initial documents. Each
value is either an ordinary unqualified selector or a canonical catalog ID such
as `manual/1/git`, `documents/mant`, or `sources/pwsh/Get-Item`. Canonical IDs
are recommended because they preserve source and manual-section identity
without widening every tool schema. MCP does not accept arbitrary local paths.

`tools/list` remains the normative input shape: collections are JSON arrays and
numeric or Boolean fields use native JSON scalars. At the MCP transport boundary
only, ManT also tolerates a bare collection item, a stringified JSON array, or a
stringified numeric or Boolean scalar. This narrow compatibility normalization
handles clients that stringify generated tool arguments; it does not widen the
native protocol, CLI JSON, or engine contracts. New clients should always emit
the canonical schema form.

Every successful tool call begins with the same stateless character-page
header:

```text
[mant-page chars=0..16384 totalChars=42137 nextChar=16384]
```

`startChar` is a zero-based Unicode scalar offset and defaults to zero.
`maxChars` is the maximum number of Unicode scalar values returned, defaults
to 16,384, and accepts 1 through 32,768. The `chars=A..B` range is half-open;
`nextChar`, when present, is exactly `B`. `totalChars` counts the complete
canonical text generated from all non-page inputs. The header and the blank
line separating it from the body are framing and do not contribute to these
coordinates. A start at or beyond the current end returns an empty body with
`chars=totalChars..totalChars`.

Because paging counts Unicode scalar values rather than bytes, a maximum-size
body occupies at most 131,072 UTF-8 bytes before JSON escaping (32,768 scalars
at four bytes each). The page header, blank separator, JSON-RPC envelope, and
any JSON escaping are framing outside that body bound. This is intentionally
not the former 32 KiB byte contract; scalar coordinates cannot split UTF-8.

Paging is deliberately stateless. Each call reruns the same base query against
the local files visible at that time, renders its complete UTF-8 text, and then
applies `startChar` and `maxChars`. The server retains no cursor, result cache,
or session snapshot, so files changing between calls may also change
`totalChars` and the meaning of an old offset. Unicode scalar indexing always
produces valid UTF-8, but a page is a text or CommonMark fragment and need not
be an independently complete Markdown construct or grapheme cluster.

Semantic query bounds remain separate from text paging. `mant_find`
materializes at most `maxResults` matching catalog rows, default 50;
`mant_search` materializes at most `maxMatches` complete occurrences, default
20. `maxResults` accepts 1 through 10,000; `maxMatches` accepts 1 through
100 because each occurrence retains preview, coordinates, and context data.
Their compact bodies report returned and total match counts, while `totalChars`
describes only the canonical body produced under the requested semantic bound.
Rows or matches excluded by `maxResults` or `maxMatches` cannot be reached by
advancing `startChar`; increase the semantic bound or narrow the query first.
The independent `offset` skips matching catalog rows or global search hits
before materialization. The compact search status names its count
`totalOccurrences`; every search scope counts complete occurrences. When more
remain it returns `nextOffset`. Callers
rerun the same non-page query with that value. Character paging is applied
afterward and continues only with `nextChar` as `startChar`.

For explain and search, `followLinks: true` expands typed manual and registered
Markdown links with the same deterministic breadth-first traversal as the
native scope contract. `maxDepth` defaults to 8 and is capped at 32;
`maxDocuments` defaults to 64 and is capped at 256. Both limit fields require
`followLinks: true`, and `maxDocuments` must include every initial document.
The compact result omits the graph itself. When `followLinks` is true, or when
an initial document is unresolved, it ends with this stable status form:

```text
[scope: documents=N, unresolved-roots=R, unresolved-links=L, depth-frontier=D, document-frontier=G, content-frontier=C]
```

All six fields are always present in that order. A complete traversal therefore
still emits the line with zero unresolved and frontier counts, making link
following observable. `R` counts unresolved initial selectors, `L` counts
unresolved followed links, and the three frontier fields count logical links
excluded by the corresponding bound. `document-frontier` therefore counts
links blocked by the configured document-count bound, while `content-frontier`
counts links blocked by the fixed 64 MiB normalized-IR budget; neither field
encodes the limit value itself. `mant_explain` additionally emits its outcome,
owner totals, all four class total/returned counts, global offset/nextOffset, source coverage and
separate candidate/relation/content truncation flags. A readable source with
no evidence is not a failure. When no evidence is found, inspect one document with
`mant_outline(document=..., entries={"kind":"all"})`, repeat for the remaining
documents, or use `mant_search` for broader text retrieval. Ordinary prose
support retains its real section and block path, labelled `literal`, without
becoming a definition. `maxResults` defaults to 50 (maximum 256); `offset` skips
owners in class-then-source order, and `contentBytes` defaults to 1 MiB (maximum 4 MiB), shared by facts, previews and bodies. These
controls precede, and remain independent from, the character-page envelope.

Discover both registered Markdown and section-qualified manual pages with:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "mant_find",
    "arguments": {
      "query": "printf",
      "kind": "manual"
    }
  }
}
```

The result starts with the total match count and one compact row per document.
Each row begins with its canonical logical ID; host filesystem paths are not
exposed. `query` is a case-insensitive literal matched against leaf names and
relative or canonical paths. Shadowed Markdown candidates remain discoverable.
When an explicit source or manual section contributes no indexed documents, an
empty result says so and lists the available namespaces. An indexed scope with
no name match remains the ordinary compact `0 matches` result.
Catalog calls include up to 50 records by default. Set `maxResults` when a
larger or smaller semantic result set is useful, then use the common character
page fields to read its rendered text.

An outline tool call is:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "mant_outline",
    "arguments": {
      "document": "manual/1/bash"
    }
  }
}
```

The default `summary` projection keeps discovery compact while disclosing
entry coverage. For stateless exploration, reuse a path or ID from the current
response as `root`, then request `entries.kind = all` or a bounded kind filter
to expand only that subtree. Rediscover after the local document changes. For example, a returned shell-builtins
section can first expose only commands. Rooting preserves original paths and
IDs and omits unrelated siblings. The illustrative IDs below come from one
installed `bash(1)`; clients use the values returned by their preceding call:

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "mant_outline",
    "arguments": {
      "document": "manual/1/bash",
      "root": {"kind":"id","id":"shell-builtin-commands"},
      "entries": {
        "kind": "kinds",
        "kinds": [{"kind": "command"}]
      }
    }
  }
}
```

The returned command ID can become the root of another call that exposes its
parameters and values:

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "mant_outline",
    "arguments": {
      "document": "manual/1/bash",
      "root": {"kind":"id","id":"command-set"},
      "entries": {"kind": "all"}
    }
  }
}
```

Then read that selected node's complete content:

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "tools/call",
  "params": {
    "name": "mant_read",
    "arguments": {
      "document": "manual/1/bash",
      "selectors": [{"kind":"id","id":"command-set"}]
    }
  }
}
```

Or collect evidence for a documented name:

```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "tools/call",
  "params": {
    "name": "mant_explain",
    "arguments": {
      "documents": ["sources/windows/reg.exe"],
      "entry": "query",
      "maxResults": 20,
      "offset": 0,
      "startChar": 0,
      "maxChars": 4096
    }
  }
}
```

The same tool accepts option aliases such as `/query` and environment aliases
such as `PATH` or `$env:PATH`. Matching follows the entry's declared case
policy. All matching owners are retained as independent evidence, not an error
or first-match guess. To read exactly one original owner, pass its path or ID to
`mant_read`. `mant_explain` also accepts an exact entry coordinate, but still
collects independent matching and explicit-relationship evidence. Tool-error
text is not a versioned structured schema; clients must not parse its prose.

To finish this same semantic page, repeat all semantic arguments unchanged and
set only `startChar` to the returned `nextChar`. Concatenating these character
pages reconstructs the canonical class-ordered presentation, including counts
and previews. To request the next semantic page instead, set `offset` to
`nextOffset` and reset `startChar` to zero. For complete mentioned or omitted
content, use the returned logical document and node in `mant_read`; neither
cursor is a filesystem path or a promise that sources remain unchanged.

A structure-aware search tool call is:

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "tools/call",
  "params": {
    "name": "mant_search",
    "arguments": {
      "documents": ["manual/1/tar"],
      "pattern": "--acls",
      "syntax": "literal",
      "case": "insensitive",
      "contextLines": 1
    }
  }
}
```

Flow/TLDR visible search uses the canonical visible extractor, while Markdown
scope searches canonical addressable export bytes (the Flow/TLDR
`--format markdown --preserve-anchors` artifact); terminal wraps are not search input. It permits
zero through five context lines. `maxMatches` selects 1 through 100 complete
occurrences and defaults to 20. `offset` skips that many occurrences globally
across the ordered document scope. `mant_read` and
`mant_explain` use CommonMark; the other tools use deterministic plain text.
Occurrences on one rendered line remain separate pagination results; the text
presentation may merge overlapping context windows.
Regex `^` and `$` match canonical rendered line boundaries, and the same Unicode/UTF-8
validation applies before a document is loaded. Result offsets and character
paging are deliberately separate: use `nextOffset` to materialize another
result page, then `nextChar` to continue the current page's presentation.
This keeps model-visible results aligned with the CLI's human presentations
without ANSI escapes, duplicated JSON, schema markers, producer metadata,
physical source paths, or non-fatal diagnostics. Protocol-level and validation
failures use standard MCP error results rather than inventing a ManT error
schema.

## Client Implementation Checklist

1. Resolve the intended `mant` executable.
2. Run `mant --protocol-version --compact` and require compatible identifiers.
3. Obtain `mant --schema request` and the expected response schema, or use a
   schema catalog pinned with the executable.
4. Construct a closed `mant.request/v0.12` object.
5. Spawn `mant --request-json --format json --compact`.
6. Write one UTF-8 request and close stdin.
7. Drain stdout and stderr concurrently and apply a timeout.
8. Require status `0`, parse exactly one JSON value, and validate its exact
   response discriminator.
9. Use typed outline paths and IDs only within their current source document;
   aliases belong to explain, not read or outline-root selection.
10. For search, interpret offsets in complete occurrence order. Resolve each
    tagged `location`; visible unit keys require their response-local
    `contentProjection`, while `markdown-artifact` coordinates refer to export
    bytes. Do not confuse any of these with original source spans.

For long-lived agent integration, use `mant --mcp`, perform standard MCP
initialization, consume the generated input schemas from `tools/list`, discover
a canonical ID with `mant_find`, inspect `totalChars`, and choose subsequent
`startChar` and `maxChars` ranges until the focused result is complete. The
native `--protocol-version` describes the JSON contract and is not an MCP
output-schema version.

## See Also

[mant(1)](mant.md), [mant-ir(7)](mant-ir.md), [mant-markdown(7)](mant-markdown.md), and [mant-roff(7)](mant-roff.md)
