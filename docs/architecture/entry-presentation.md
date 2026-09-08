# Entry presentation

Presentation consumes semantic facts; it does not discover new names, aliases,
value domains or document identities. Source markup changes style, not the
meaning of an entry. A type color is neither a query match nor a confidence
level.

## Inputs and ownership

Full text and node excerpts render their materialized Block/Inline content.
Outline and search use their protocol titles and entry kinds. Explain is a
projection of its evidence response, not another selector query. The TUI builds
its semantic index and logical content once per DocumentView and performs
width-dependent layout separately.

`mant-protocol::presentation` owns terminal-neutral label modes, role families
and source-binding projection. The engine owns textual block layout; CLI and
TUI adapters own terminal colors and geometry. No ANSI, RGB or presentation
state is stored in document IR or serialized as a semantic fact.

## Labels and roles

Compact labels prefer validated names, then visible forms, then the ID. Forms
labels prefer complete visible forms before the Compact fallback. A display
separator between names does not declare equivalence. Empty names remain empty
in the semantic model, even when a form supplies the display title.

CLI outline puts path and full title first, with complete `ID:`, `Entries:` and
`Relationships:` hanging lines underneath. It never truncates to a fixed terminal
width. Plain and ANSI share that tree projection and keep continuation guides.
TUI compact labels use Compact; expanded labels use Forms. This difference is
intentional and does not imply different names or semantic identities.

Terms are primary content, not muted metadata. Values use the blue family;
parameters green; commands warm; environment variables cyan; configuration
keys yellow; variables purple/pink. The complete EntryKind, including marker
and operand subtypes, survives the mapping even where colors are shared.
Coordinates, connectors and statistics may use secondary colors independently
of the title. CLI neutral foreground inherits the terminal palette.

## Source-bound styles

`EntryStyleMap` binds original inline containers to validated name ranges once
per borrowed rendering operation. It keys actual containers, not their text:
equal words in another owner, a body mention, a list marker, or a longer word
do not acquire a semantic name role. Rejected bindings leave the original text
and source markup readable without guessing.

`project_content_slice` translates an owner-local inline path and optional
UTF-8 byte slice to half-open Unicode scalar positions in that same root.
Terms are separate roots from paragraphs; term indices are not form indices.
Anchors occupy zero positions, a source newline one, and replacing an unsafe
control scalar with U+FFFD does not move subsequent positions. Generated
indentation, list markers, table separators and report framing are outside
this coordinate domain.

`visit_inline_text` emits borrowed pieces with orthogonal Strong, Emphasis,
Code, Link and validated-name roles. Adapters apply base style, source markup,
then the more specific name foreground. Code and Link never clear inherited
bold or italic. Link affordance and its target survive type coloring. Lexical
code accents are weaker than explicit source/name roles.

Match decoration is supplied only by a reported query location. Interactive
search and selection apply their own final overlay; they do not mutate cached
styles, facts, anchors, copied text or link hit regions.

## Text adapters

The engine's plain and decorated full/node outputs share one block-layout
implementation. Decorations may add zero-width styles, but preserve source
characters, newlines and boundary whitespace so trimming, indentation and
list continuation remain identical. Dynamic metadata is sanitized as a
single line; original body text retains legitimate newlines and tabs.

Ordinary full documents and TUI text never acquire explain report prefixes.

## Explanation reports

Single-document and scope reports traverse the same returned DTO, with one
class-first record stream. Headings contain the original title and coordinates;
`Matched by` is a separate field using collected spellings and identity fields.
All four category counts remain visible, including zero and off-page counts.
Class boundaries and owner boundaries are distinct even without ANSI.

Every source line in forms, direct/related bodies and mention previews is quoted
(`| ` in text, `> ` in CommonMark), including blank lines and code fences.
Generated field labels are outside that boundary. Dynamic metadata is single-line
sanitized before decoration/escaping; source roots retain their original newlines.
Ordinary full/node output is not quoted this way.

An operation-local location map borrows only the returned forms/body. It validates
each occurrence domain atomically, normalizes bounded overlaps once, and layers
ordinary type bindings with precise query emphasis. It never searches for query
strings or falls back to whole-line highlighting. Malformed ranges lose decoration,
not content. Text/ANSI share the existing block layout; ANSI maps the resulting
roles to styles. CommonMark emphasizes exact inline matches before escaping while
retaining original wrappers. Fenced displays stay verbatim: Markdown markers inside
a fence would change code rather than highlight it. Their match coordinates remain
available in the DTO and text/ANSI output.
Plain/ANSI equality is necessary but not sufficient: tests also assert the
actual styled positions and absence of style on misleading prefix matches.
