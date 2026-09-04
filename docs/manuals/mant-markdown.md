# mant-markdown

## Name

mant-markdown — supported Markdown dialect and semantic extensions for ManT documents

## Description

ManT parses `.md` and `.markdown` input with `pulldown-cmark` 0.13, then lowers a deliberately conservative subset into [mant-ir(7)](mant-ir.md). Supported constructs become semantic nodes shared with native manuals. Recognized but unsupported constructs remain visible as exact source text and produce structured diagnostics.

This preservation rule keeps a document readable without pretending that unsupported presentation or interaction semantics were understood.

## Document Structure

Headings from H1 through H6 form a recursive section tree. The first heading, when it is H1, becomes the document title and is removed from the visible section tree. Content before the first remaining section is stored as document-overview blocks.

Heading levels determine ancestry. Skipped levels are accepted; depth follows the nearest preceding heading with a lower level. Duplicate titles receive distinct document-local IDs.

An explicit heading ID written as a whitespace-separated final `{#configuration}` becomes an exact fragment alias. If it already satisfies the normalized ID grammar and is not a reserved selector, it is also the heading's internal ID; otherwise the internal ID is derived from the visible title while the exact authored fragment remains usable. This is the complete supported heading-attribute grammar: class-only blocks, custom key/value attributes, attached brace groups, and ordinary trailing brace text are retained as title text rather than consumed. Consequently headings such as `GET /users/{id}` and `Route /users/{#id}` preserve the path parameter. Internal IDs use lower-case Unicode alphanumeric characters and underscores, replace other runs with `-`, and receive numeric suffixes when needed. A local link may use the exact authored alias, while outlines and structured selectors expose the normalized ID.

## Supported Blocks

| Markdown construct | IR result | Notes |
| --- | --- | --- |
| Paragraph | `paragraph` | Source soft breaks become spaces |
| ATX or Setext heading | `section` | H1 through H6 |
| Indented code block | `preformatted` | No language |
| Fenced code block | `preformatted` | First info-string word is the language |
| Bullet list | `list` | Loose and tight items retain block structure |
| Ordered list | `list` | Explicit starting number is retained |
| Nested list | Nested blocks | Maximum semantic nesting depth is 64 |
| GFM pipe table | `table` | Left, center, and right alignment retained |
| Thematic break | `thematic-break` | Semantic separator |

Lists may contain paragraphs, code blocks, tables, and nested lists. Content deeper than the recursion budget is preserved as unsupported source rather than recursed into indefinitely.

## Supported Inline Syntax

| Markdown construct | IR result | Notes |
| --- | --- | --- |
| Plain text | `text` | UTF-8 is preserved |
| Code span | `code` | Literal content |
| Strong emphasis | `strong` | Nested content retained |
| Emphasis | `emphasis` | Nested content retained |
| Soft break | Text space | Source wrapping does not change prose |
| Hard break | `line-break` | Backslash or two-space break |
| Link | Typed `link` | Destination classification described below |

Strong and emphasis spans may nest to depth 64. Deeper spans remain visible with an unsupported diagnostic.

## Links

Markdown links are classified before entering the IR:

| Destination | Link target |
| --- | --- |
| `#fragment` | Section in the current document |
| `mailto:user@example.com` | Email address |
| `other.md` or `guide/other.markdown#part` | Registered Markdown document |
| Other ordinary URI | External URI |

URI scheme classification is ASCII case-insensitive, so a valid single-address
`MAILTO:` and `mailto:` without headers or a fragment both produce a typed
email target. Percent escapes in that address are decoded exactly once before
mailbox validation, and consumers use the shared inverse serializer when they
activate the typed target. Recipient lists and mailto URIs with a query or
fragment remain external URIs so their complete action is preserved.
Structurally invalid external and email targets remain visible and receive an
IR diagnostic rather than becoming trusted activation requests.
External URI components use ASCII RFC 3986 syntax and complete `%HH` escapes.
Typed email addresses accept an ASCII dot-atom local part and conservative DNS
domain. URI-sensitive mailbox characters such as `%` and `/` are percent-
encoded during activation; quoted, internationalized, leading-dot, trailing-
dot, and consecutive-dot local parts remain visible but invalid.

Relative document links retain an extension-free logical path. `.md` and `.markdown` matching is case-insensitive. Absolute paths, query strings, control characters, and non-Markdown suffixes do not become document-navigation links.

Paths containing `.` or `..` components are represented but navigation remains constrained to the current registered document source. A document link cannot escape its source boundary. Unresolved local fragments remain visible and are diagnosed rather than silently redirected.

Wiki links are not part of the supported link contract.

## Semantic Entry Lists

ManT uses definition identities to make options, markers, operands, commands, configuration keys, environment variables, variables, values, and terms directly addressable by `--explain`, outlines, the TUI, JSON, and MCP. Markdown has no portable definition-list syntax, so ManT provides an invisible directive for a complete bullet list:

```markdown
<!-- mant:entries role=option case=sensitive -->
- `-h`, `--help`: Show help.
- `--color` _WHEN_: Select color output.
```

The directive must be the only construct on its line and immediately precede a complete bullet list. Required fields are:

| Field | Values | Meaning |
| --- | --- | --- |
| `role` | `option`, `marker`, `operand`, `command`, `configuration-key`, `environment-variable`, `variable`, `value`, `term` | Entry semantics |
| `case` | `sensitive`, `insensitive` | Alias lookup policy |
| `attached` | `infer`, `fixed` | Optional option-value policy |

`attached` applies only to option entries. With `infer`, a declaration such as `` `--output=FILE` `` exposes `--output` and accepts attached values. With `fixed`, punctuation remains part of the exact option name. This is useful for real Windows tokens such as `-ca.cert`. An explicitly declared negated dash option may prefix a valid `-` or `--` spelling with `!`, for example `!--reloadEnvironment`; arbitrary `!name` tokens are not options.

The other roles preserve complete authored names. Use `marker` for parser-control tokens such as `--`, `operand` for positional or special operands, `configuration-key` for named configuration-language keys, `value` for a value accepted by a parent entry, and `term` only when no more specific reliable role applies.

Environment-variable declarations use one cross-platform name grammar shared with native manuals. Accepted spellings are bare names such as `PATH`, shell references such as `$PATH`, PowerShell provider references such as `$Env:PATH` and `${Env:ProgramData}`, and Windows references such as `%ProgramFiles(x86)%`. Provider matching is ASCII case-insensitive. An assignment term such as `RUST_LOG=debug` exposes `RUST_LOG` as its selector while preserving the complete assignment as an authored form. A name starts with an ASCII letter or underscore and then uses ASCII letters, digits, `_`, `-`, or parentheses. The directive supplies the semantic context; ManT never scans ordinary prose for name-shaped words.

Each list item must begin with one or more code spans containing names and then an explicit description delimiter. Ambiguous, malformed, mixed-purpose, or colliding declarations remain ordinary lists and produce author-facing diagnostics instead of silently losing selectors.

A term may instead be one document link wrapping exactly one code span:

```markdown
<!-- mant:entries role=command case=insensitive -->
- [`winget.exe`](winget.exe.md): Open the Windows package manager manual.
```

The linked code remains the selectable name and the relative Markdown target becomes an explicit semantic document destination. A link in the description remains ordinary reference material; it does not change the entry destination. External links, section links, linked prose, and links wrapping mixed inline content are not accepted as semantic terms.

Declared entry lists may nest at any Markdown list depth within the parser's 64-level structural budget. Every nested list that needs a semantic role has its own immediately preceding `mant:entries` directive; the derived index preserves parent → child ownership rather than flattening it.

An entry whose accepted values are entries in another document can declare that relationship inside its list item:

```markdown
<!-- mant:entries role=option case=sensitive -->
- `-o OPTION`: Set an SSH configuration key.

  <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->
```

`mant:domain` must be the exact directive name, be the only construct on its
line, and be structurally contained by the semantic list item it describes.
Its attachment follows the CommonMark item rather than a particular source-line
layout, so the list marker, leading term, blank lines, and directive may occupy
separate lines. `entries` accepts one relative `.md` or `.markdown` document,
or an exact `manual/<section>/<name>` path whose section follows the native
manual-section grammar. It addresses the complete target document, so
fragments are rejected. `roles` is a non-empty comma-separated list drawn from
the same roles as `mant:entries`; repeating a role is an error. Unknown,
duplicate, malformed, or unattached declarations produce
`markdown.semantic-value-domain`, leave document content visible, and make the
semantic projection incomplete. A syntactically valid reference remains useful
even when catalog lookup is unavailable; resolution is an engine/protocol
concern rather than a Markdown parsing requirement.

Ordinary option-shaped definition lists produced by native manuals can receive identities automatically. Markdown lists require either the explicit directive or the conservative complete-list inference described in the shipped examples; authors should use the directive when role or case policy matters.

An accepted list item remains a normal definition item in the document tree;
the directive adds its source-neutral `DefinitionIdentity`. From those content
facts, `SemanticIndex` derives entry kinds, selector aliases, complete authored
forms, explicit document targets, value domains, and nested ownership. Outline, excerpt, explanation,
TUI, and MCP projections consume that derived index rather than reparsing the
Markdown list. See [mant-ir(7)](mant-ir.md) for the distinction between content
definitions and indexed concepts.

Links follow the same source-to-IR boundary: a fragment becomes a local section
target, a relative Markdown path becomes a same-source document edge, and web
or email destinations remain host actions. Linked entry terms and entry-set
domains join ordinary typed document links in bounded multi-document
traversal; the Markdown parser does not perform catalog or filesystem lookup
while classifying the reference. Scope traversal orders linked terms,
description links, and value-domain declarations by their authored source
positions; declaring a domain later in an item does not move it ahead of an
earlier visible link when a document budget truncates traversal.

## Embedded tldr

A document may own one tldr-compatible quick reference before its ordinary Markdown body:

```markdown
<!-- mant:tldr:start -->
# tool

> One-line quick reference.

- Run the tool:

`tool {{file}}`
<!-- mant:tldr:end -->

# Tool
```

The opening marker must be the first non-empty construct and must have a closing marker. The enclosed page follows the tldr-pages layout: one H1 title, block-quote description paragraphs, and description/command example pairs. `{{placeholder}}` command fragments are retained for terminal styling.

The boundary comments are invisible to ordinary CommonMark renderers. ManT masks the complete preface before parsing the manual body so source byte offsets and line numbers remain stable. Embedded content has `embedded` provenance and does not claim the community cache license.

## Preserved Unsupported Syntax

The parser recognizes several CommonMark or GFM extensions that ManT does not assign semantic IR nodes. Their source remains visible in an `unsupported` block or text run with a diagnostic:

| Construct | Preservation behavior |
| --- | --- |
| Block quote | Complete source block |
| Raw HTML block or span | Exact source |
| Image | Exact Markdown source, not fetched |
| Task list | Complete list source |
| Footnote definition or reference | Exact source |
| Definition list extension | Complete source block |
| Math span or block | Delimiters retained; CommonMark punctuation escapes become their visible characters |
| Strikethrough | Exact source |
| Superscript or subscript | Exact source |
| Wiki link | Exact source |
| YAML or plus-delimited metadata | Complete source block |

Preserved source is rendered as visible text, not interpreted HTML, executable code, remote media, or mathematics. This behavior is intentionally safe and deterministic.

## Input Safety

A leading UTF-8 byte-order mark is masked so it cannot hide the embedded tldr marker or demote the first heading. Terminal-unsafe control characters are replaced with spaces. Both cases produce diagnostics while preserving source offsets.

Markdown parsing never executes HTML, follows remote links, loads images, or reads linked local files. Cross-document navigation resolves only through ManT's registered catalog.

## Authoring Guidance

Use one H1 document title followed by H2 manual sections and deeper headings only where they improve navigation. Prefer paragraphs, fenced code, ordinary lists, semantic entry lists, and tables. Use explicit relative `.md` links for cross-document navigation and explicit heading IDs only when a stable human-authored fragment is important.

Use these focused checks while authoring:

```sh
mant --input ./tool.md --outline --outline-entries all --format json --compact
mant --input ./tool.md --search warning --word --context 1 --format markdown
```

The first command exposes the addressable outline and any diagnostics in one
machine-readable result. The second verifies how a reader sees a specific term
with its surrounding context. An empty `diagnostics` array confirms that the
document stayed inside the supported semantic subset.

## See Also

[mant(1)](mant.md), [mant-ir(7)](mant-ir.md), [mant-protocol(5)](mant-protocol.md), [mant-roff(7)](mant-roff.md), and the [CommonMark specification](https://spec.commonmark.org/)
