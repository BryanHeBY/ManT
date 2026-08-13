# mant-markdown

## Name

mant-markdown — supported Markdown dialect and semantic extensions for ManT documents

## Description

ManT parses `.md` and `.markdown` input with `pulldown-cmark` 0.13, then lowers a deliberately conservative subset into [mant-ir(7)](mant-ir.md). Supported constructs become semantic nodes shared with native manuals. Recognized but unsupported constructs remain visible as exact source text and produce structured diagnostics.

This preservation rule keeps a document readable without pretending that unsupported presentation or interaction semantics were understood.

## Document Structure

Headings from H1 through H6 form a recursive section tree. The first heading, when it is H1, becomes the document title and is removed from the visible section tree. Content before the first remaining section is stored as document-overview blocks.

Heading levels determine ancestry. Skipped levels are accepted; depth follows the nearest preceding heading with a lower level. Duplicate titles receive distinct document-local IDs.

An explicit heading attribute such as `{#configuration}` becomes a link alias. Automatic IDs use lower-case Unicode alphanumeric characters and underscores, replace other runs with `-`, and receive numeric suffixes when needed. Reserved selectors are moved out of the selector namespace while retaining their source alias.

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

Relative document links retain an extension-free logical path. `.md` and `.markdown` matching is case-insensitive. Absolute paths, query strings, control characters, and non-Markdown suffixes do not become document-navigation links.

Paths containing `.` or `..` components are represented but navigation remains constrained to the current registered document source. A document link cannot escape its source boundary. Unresolved local fragments remain visible and are diagnosed rather than silently redirected.

Wiki links are not part of the supported link contract.

## Semantic Entry Lists

ManT uses definition identities to make options, commands, variables, and environment variables directly addressable by `--explain`, outlines, the TUI, JSON, and MCP. Markdown has no portable definition-list syntax, so ManT provides an invisible directive for a complete bullet list:

```markdown
<!-- mant:entries role=option case=sensitive -->
- `-h`, `--help`: Show help.
- `--color` _WHEN_: Select color output.
```

The directive must be the only construct on its line and immediately precede a complete bullet list. Required fields are:

| Field | Values | Meaning |
| --- | --- | --- |
| `role` | `option`, `command`, `environment-variable`, `variable` | Entry semantics |
| `case` | `sensitive`, `insensitive` | Alias lookup policy |
| `attached` | `infer`, `fixed` | Optional option-value policy |

`attached` applies only to option entries. With `infer`, a declaration such as `` `--output=FILE` `` exposes `--output` and accepts attached values. With `fixed`, punctuation remains part of the exact option name. This is useful for real Windows tokens such as `-ca.cert`.

Each list item must begin with one or more code spans containing names and then an explicit description delimiter. Ambiguous, malformed, mixed-purpose, or colliding declarations remain ordinary lists and produce author-facing diagnostics instead of silently losing selectors.

Ordinary option-shaped definition lists produced by native manuals can receive identities automatically. Markdown lists require either the explicit directive or the conservative complete-list inference described in the shipped examples; authors should use the directive when role or case policy matters.

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
| Math span or block | Exact source |
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

Run these checks while authoring:

```sh
mant --input ./tool.md --outline=entries
mant --input ./tool.md --format json --compact
mant --input ./tool.md --search warning
```

An empty `diagnostics` array confirms that the document stayed inside the supported semantic subset.

## See Also

[mant(1)](mant.md), [mant-ir(7)](mant-ir.md), [mant-protocol(5)](mant-protocol.md), [mant-roff(7)](mant-roff.md), and the [CommonMark specification](https://spec.commonmark.org/)
