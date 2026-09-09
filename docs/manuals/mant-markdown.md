# mant-markdown

## Name

mant-markdown — supported Markdown dialect and semantic extensions for ManT documents

## Description

ManT parses `.md` and `.markdown` input with `pulldown-cmark` 0.13, then lowers a deliberately conservative subset into [mant-ir(7)](mant-ir.md). Supported constructs become semantic nodes shared with native manuals. Recognized but unsupported constructs remain visible as exact source text and produce structured diagnostics.

This preservation rule keeps a document readable without pretending that unsupported presentation or interaction semantics were understood.

## Document Structure

Headings from H1 through H6 form a recursive section tree. The first heading, when it is H1, moves to the document's visible `heading` and is removed from the section tree, without discarding its content or duplicating it in metadata. Content before the first remaining section is stored as document-overview blocks. A heading-only document remains readable.

ATX and Setext headings retain their real inline content, including links, nested emphasis, code, anchors and supported hard breaks. A linked title is not automatically a command or another semantic entry. Plain outline labels and IDs derive from visible words, not from link destinations; changing only a destination does not change the heading's local ID.

Markdown export uses Setext syntax for level-one/two headings containing hard breaks. Deeper ATX headings fold those breaks to spaces while retaining the level, words and links; the IR retains the original breaks.

Heading links retain their typed destinations in Markdown output. When a heading links to a local target, export automatically retains addressable HTML anchors, including the document-root target before its visible H1. This addressable fallback takes priority over optional semantic comments: otherwise dropping a non-heading target could leave the exported heading link dangling. Raw anchor HTML is not part of the semantic reimport subset; use IR JSON when every content and navigation fact must round-trip.

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

List tightness follows the parser's direct item structure: a loose nested list does not make its tight parent loose. LF, CRLF and CR are equivalent line endings; an empty line or one containing only spaces or tabs counts as a blank line. Original byte ranges and physical source-line coordinates remain available in IR spans. Semantic declarations are consumed within the original event tree, so removing a declaration cannot merge two independently authored lists.

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
| `man:printf(3)` or `man:printf` | Native manual, with explicit or unresolved section |
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

Relative document links retain an extension-free logical path. `.md` and `.markdown` matching is case-insensitive. URI schemes and network authorities are classified first, even when a host or URI path ends in `.md`. Absolute paths, query strings, control characters, and non-Markdown suffixes do not become document-navigation links.

Percent-encoded local path components and fragments are decoded exactly once as UTF-8 before logical-address validation. For example, `space%20name.md#Mixed%2ETarget` addresses `space name` and fragment `Mixed.Target`; `%2520` instead represents literal `%20`. Invalid escapes, invalid UTF-8, controls, and encoded path separators or query/fragment delimiters do not create document-navigation links. Rendered Markdown re-encodes logical components, including literal percent signs. External URI escapes are preserved, not decoded as local paths.

Paths containing `.` or `..` components are represented but navigation remains constrained to the current registered document source. A document link cannot escape its source boundary. Unresolved local fragments remain visible and are diagnosed rather than silently redirected.

Decoded fragments match exact authored aliases or normalized IDs. Resolution does not strip a second `#`, trim whitespace, change case, or generate a fallback slug: `#%23foo` addresses the alias `#foo`, not `foo`.

Wiki links are not part of the supported link contract.

The explicit `man:` destination preserves typed native references when heading
content is exported back to Markdown. The name and optional parenthesized
section use the shared manual-reference validation rules; URI components are
decoded once. Omitting a section does not select section 1 implicitly. Heading
export retains these links; ordinary body presentation keeps its existing
compact manual-reference policy.

## Semantic Entry Lists

### Content and annotation boundary

The design principle is that ordinary Markdown supplies the visible content
and ManT semantic comments add facts about it. An annotation must not invent
visible names, reorder items, or change punctuation, paragraph boundaries,
list nesting or hard-break semantics. This concerns content structure, not
identical pixels across renderers or byte-identical Markdown round trips.
Comment placement still follows CommonMark: inserting a block comment or
blank line can itself change the parsed structure.

Accepted semantic items remain ordinary list items. Their code terms, `:`,
dash and pipe delimiters, paragraphs, numbering and tightness are preserved;
read-only form/name bindings point into that same content. The supported
syntax below includes list declarations and optional item-owned `mant:entry`
JSON metadata. Neither kind of annotation supplies replacement body text.

### Current declarations

ManT attaches entry facts to make options, markers, operands, commands, configuration keys, environment variables, variables, values, and terms directly addressable by `--explain`, outlines, the TUI, JSON, and MCP. An invisible directive supplies role and case policy for an ordinary bullet or ordered list:

```markdown
<!-- mant:entries role=option case=sensitive -->
- `-h`, `--help`: Show help.
- `--color WHEN`: Select color output.
```

The directive must be the only construct on its line and immediately precede a list. `role` and `case` are required; `attached` is optional. Each explicitly declared item is validated independently: an invalid head remains ordinary visible content and produces a diagnostic, while valid siblings retain their entries. Without a directive, conservative option recognition still requires a complete option-shaped bullet list; ordered lists are never inferred:

| Field | Values | Meaning |
| --- | --- | --- |
| `role` | `option`, `marker`, `operand`, `command`, `configuration-key`, `environment-variable`, `variable`, `value`, `term` | Entry semantics |
| `case` | `sensitive`, `insensitive` | Alias lookup policy |
| `attached` | `infer`, `fixed` | Optional option-value policy |

`attached` applies only to option entries. With `infer`, a declaration such as `` `--output=FILE` `` exposes `--output` and accepts attached values. With `fixed`, punctuation remains part of the exact option name. This is useful for real Windows tokens such as `-ca.cert`. An explicitly declared negated dash option may prefix a valid `-` or `--` spelling with `!`, for example `!--reloadEnvironment`; arbitrary `!name` tokens are not options.

The other roles preserve complete authored names. Use `marker` for parser-control tokens such as `--`, `operand` for positional or special operands, `configuration-key` for named configuration-language keys, `value` for a value accepted by a parent entry, and `term` only when no more specific reliable role applies.

Environment-variable declarations use one cross-platform name grammar shared with native manuals. Accepted spellings are bare names such as `PATH`, shell references such as `$PATH`, PowerShell provider references such as `$Env:PATH` and `${Env:ProgramData}`, and Windows references such as `%ProgramFiles(x86)%`. Provider matching is ASCII case-insensitive. An assignment term such as `RUST_LOG=debug` exposes `RUST_LOG` as its selector while preserving the complete assignment as an authored form. A name starts with an ASCII letter or underscore and then uses ASCII letters, digits, `_`, `-`, or parentheses. The directive supplies the semantic context; ManT never scans ordinary prose for name-shaped words.

Each list item must begin with one or more code spans containing names and then an explicit description delimiter. Ambiguous, malformed, mixed-purpose, or colliding declarations remain ordinary lists and produce author-facing diagnostics instead of silently losing selectors.

Separate selectable names within one displayed form with commas. Use a
standalone `|` between code terms to separate complete invocation forms of the
same entry:

```markdown
<!-- mant:entries role=option case=sensitive -->
- `-o FILE`, `--output FILE` | `--output=FILE`: Write output.
```

This produces aliases `-o` and `--output`, and two forms: `-o FILE, --output FILE`
and `--output=FILE`. All aliases select the same definition and description.
The pipe must be outside code spans and links, with a term on both sides;
empty forms reject that item's annotation without changing its content or valid siblings. A pipe inside a
code span remains part of that authored form and follows its role's name grammar.

A term may instead be one document link wrapping exactly one code span:

```markdown
<!-- mant:entries role=command case=insensitive -->
- [`winget.exe`](winget.exe.md): Open the Windows package manager manual.
```

The linked code remains the selectable name and the relative Markdown target becomes an explicit semantic document destination. A link in the description remains ordinary reference material; it does not change the entry destination. External links, section links, linked prose, and links wrapping mixed inline content are not accepted as semantic terms.

Declared entry lists may nest at any Markdown list depth within the parser's 64-level structural budget. Every nested list that needs a semantic role has its own immediately preceding `mant:entries` directive; the derived index preserves parent → child ownership rather than flattening it.

An entry can explicitly state whether its locally listed values are complete:

```markdown
<!-- mant:entries role=option case=sensitive -->
- `--color WHEN`: Select color behavior.

  <!-- mant:domain choices=exhaustive -->

  <!-- mant:entries role=value case=sensitive -->
  - `auto`: Decide automatically.
  - `always`: Always use color.
  - `never`: Never use color.
```

`choices=exhaustive` is the author's assertion that the listed choices are the
complete value space. `choices=open` explicitly leaves that space open.

An exhaustive declaration is omitted with a source-located diagnostic if any direct child entry or child-list declaration was rejected. Successful siblings and all original content remain intact; they may still support open choices. Ordinary unannotated containers, including nested bullet and ordered list items, pass child-entry and declaration failures to their nearest semantic owner. Coverage stops at each successfully recognized child owner, so an unrelated warning or a rejection inside that child's own body does not invalidate this parent's enumeration. Open choices never assert that rejected or undiscovered values do not exist.
Both require at least one direct semantic child and require every direct
semantic child to have `role=value`. Ordinary descriptive paragraphs are not
choices. The parser cannot prove exhaustiveness against an executable; it
validates the declaration's structure, not the program's behavior. Without a
domain declaration, all-value children still infer non-exhaustive choices.

Declaration ownership uses the original parser's list and item positions;
removing a leading semantic comment never reassigns its domain or failure
state to another content block or item.

An entry whose accepted values are entries in another document can instead
declare that relationship inside its list item:

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
semantic projection incomplete. Multiple valid declarations on one entry are
ambiguous rather than first- or last-wins: ManT attaches none of the competing
domain declarations and does not traverse their references. Independently
observed all-value children may still infer open choices, never an exhaustive
claim. The `choices` form cannot be combined with `entries` or `roles`.
A syntactically valid reference
remains useful when it is the entry's only declaration even if catalog lookup
is unavailable; resolution is an engine/protocol concern rather than a
Markdown parsing requirement.

Ordinary option-shaped definition lists produced by native manuals can receive identities automatically. Markdown lists require either the explicit directive or the conservative complete-list inference described in the shipped examples; authors should use the directive when role or case policy matters.

An accepted list item stays in the document tree and receives source-neutral
`EntryFacts` in its optional `entry` field. From those content
facts, `SemanticIndex` derives entry kinds, selector aliases, complete authored
forms, explicit document targets, value domains, and nested ownership. Outline, excerpt, explanation,
TUI, and MCP projections consume that derived index rather than reparsing the
Markdown list. See [mant-ir(7)](mant-ir.md) for the distinction between content
definitions and indexed concepts.

### Authoring-to-entry field map

| Semantic entry field | Authoring source |
| --- | --- |
| `id` | Derived by default; optional `mant:entry` JSON `id` selects an exact validated identity. Heading `{#id}` attributes still address headings, not entries. |
| `kind` | `role=` on the owning list; option, marker, and operand map to parameter kinds. |
| `case` | Required `case=` on the owning list. |
| `names` | Selectable names extracted from the visible code terms, including linked code; grouped terms select shared content, not necessarily equivalent behavior. |
| `forms` | Complete leading terms, including placeholders; an outside-code `\|` splits independent forms without creating another entry. |
| `documentTargets` | Typed document links wrapping a code term; links in the description remain ordinary references. |
| `children` | Structurally nested semantic lists with their own role and case declarations. |
| `valueDomain` | Explicit `mant:domain choices=...` or `entries=... roles=...`; otherwise all-value children infer open choices. |
| `aliasGroups` | Optional `mant:entry` JSON groups, each grounded in two or more disjoint visible names. |
| `aliasOf` | Optional `mant:entry` JSON same-document entry ID; a validated relation between independent owners, not content redirection. |

There are no independent `aliases=` or `forms=` attributes. This keeps indexed
spellings and invocation forms grounded in content visible to human readers.
For example, one `` `cd`, `chdir` `` item in a declared command list creates
one command with two selectable aliases. Likewise ManT's own manual groups
`` `--search PATTERN`, `--grep PATTERN` `` into one option entry; explaining
either alias returns the same full description. Place genuinely different
commands in separate items even when their descriptions happen to be similar.

The `names` field describes lookup, not a verified equivalence
relation. A common description alone does not prove that two options are
interchangeable or accept the same argument syntax. ManT does not infer that
claim from commas, shared prose or the number of forms.

### Explicit item relationships

Inside an explicitly declared list, an item may carry one metadata object:

```markdown
<!-- mant:entries role=option case=sensitive -->
- `-h`, `--help`: Show help. <!-- mant:entry {"id":"help","aliasGroups":[["-h","--help"]]} -->
- `-S`, `--since`, `-U`, `--until`: Set bounds. <!-- mant:entry {"id":"bounds","aliasGroups":[["-S","--since"],["-U","--until"]]} -->
- `--data-ascii <data>`: Another data name. <!-- mant:entry {"id":"ascii","aliasOf":"data"} -->
- `-d <data>`, `--data <data>`: Submit data. <!-- mant:entry {"id":"data","aliasGroups":[["-d","--data"]]} -->
```

The object has only optional `id`, `aliasGroups`, and `aliasOf` fields. Role and case come from the list; value domains keep their separate declaration. Place the comment at the end of the item's first paragraph, or as a single-line standalone comment directly inside that item. Nested items own their declarations; metadata never borrows an owner across a block quote, code block, link, or another container. Code-span, fenced-code, and link-destination examples do not activate declarations. Conservative undeclared option recognition does not authorize metadata.

Each object is limited to 8192 UTF-8 bytes on one physical line, with at most 32 groups and 32 members per group. The closed shape permits no arbitrary JSON nesting. Invalid JSON, types (including explicit null), unknown or duplicate keys, competing objects, and object-limit violations reject the entire object. All recognized metadata comments remain non-visible, including rejected and unclosed comments; unclosed input follows the original parser event boundary rather than being reconstructed across events. Escape comment terminators inside JSON strings, for example as `\u002d\u002d\u003e`.

An explicit ID must satisfy the canonical ID grammar, fit 512 Unicode scalars, avoid reserved selectors, and be unique. Invalid or duplicated declarations retain the derived identity with a diagnostic; they are not silently renamed. `aliasGroups` members use the exact visible name spelling, not complete argument forms: `-o FILE` and `--output=FILE` contribute `-o` and `--output`. Explicit `<data>` placeholders may use lowercase; unbracketed placeholders retain the uppercase convention. Groups have at least two uniquely bound names, cannot overlap under the entry's case policy, and have no implied canonical first member. Partial grouping is allowed; ungrouped names remain independent subjects. An invalid group rejects the complete `aliasGroups` field, not otherwise valid metadata or body content.

`aliasOf` supports forward references to same-document IDs only. Both owners must have the same role and case policy and each describe one name or one group covering all their names. Missing, duplicate, ambiguous multi-subject, incompatible, self-referential, and cyclic relationships are rejected without first/last-wins behavior. A rejected relation does not erase an independent ID or valid group. The shared IR relationship validator is also the authoring validator. Relationships never merge owners, change nesting, copy descriptions, inherit value domains, execute examples, or expand filesystem/network authority.

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

### Semantic export subset

For each annotated list, export proves that either the default attached-value policy or `attached=fixed` reconstructs the same final names, forms and visible-name bindings. It emits `attached=fixed` only when that policy is required for the whole list. Lists mixing incompatible policies fall back to ordinary Markdown; neither fixed suffixes nor placeholders may silently change meaning. Nested lists select their policies independently.

The Rust renderer's `MarkdownOptions.preserve_semantics` opt-in emits list declarations, item IDs, explicit alias groups, same-document aliasOf relationships and supported value-domain comments. It supports documents whose annotated owners are ordinary lists containing only successfully declared items with the same role/case within each list. Nested lists are checked independently. Relation comments escape HTML delimiter characters in JSON strings. Reimport rebuilds bindings against the new content; original source spans are not retained.

Documents with native definition owners, inferred or mixed/partly rejected lists, invalid IR facts, or an entry-set reference without a representable document destination fall back to ordinary portable Markdown without semantic comments. The same whole-document fallback applies when valid IR metadata exceeds the authoring limits: 32 alias groups, 32 members per group, 512 characters per explicit ID, or 8192 bytes of escaped JSON per item. Import and export share these checks; they are Markdown representation limits, not general IR limits. Roff shared names never manufacture alias groups. Except for the heading-local-link fallback described above, semantic export takes precedence over raw HTML anchor export when both options are set: entry IDs are carried by metadata, not injected into the head. Heading IDs, arbitrary native layout, unsupported containers and exact source bytes are outside this subset. Use IR JSON for a complete facts/bindings serialization; Markdown is not a lossless semantic round trip.

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

Generated Markdown is a presentation of IR, not a lossless serialization of
the original source. It can be read again with `--input`, but parse/render
cycles need not be byte-identical: source wrapping, list continuation layout,
escaping, and source-specific semantic annotations can change. Search
coordinates refer to the exact generated output from the same query, not to
the output of a later parse/render cycle. Use structured IR for semantic
inspection and retain the original generated Markdown to reuse its offsets.

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
with its surrounding context. An empty `diagnostics` array means no findings
were reported by the implemented checks; it does not prove that all names
were discovered or that the executable's behavior was fully modeled.

## See Also

[mant(1)](mant.md), [mant-ir(7)](mant-ir.md), [mant-protocol(5)](mant-protocol.md), [mant-roff(7)](mant-roff.md), and the [CommonMark specification](https://spec.commonmark.org/)
