# Crate compatibility changelog

This changelog records public API, semantic-compatibility, and migration notes
for ManT's independently versioned Rust crates. Internal refactors and tests
are omitted unless they change an observable contract. Product-level features,
installers, and native artifacts remain documented in the curated
[GitHub Releases](https://github.com/BryanHeBY/ManT/releases).

Version `0.9.0` was the final lockstep publication. Later entries name the
affected crate and version explicitly; absence of a crate from an entry means
that crate was not published for that change.

## Unreleased

### Shared entry presentation

- Keep generic Term titles and names in the primary foreground instead of
  treating them as muted metadata. CLI and TUI share semantic palette families
  and stable kind labels, with Value distinct from Term and Match distinct from
  command color. Palette choices do not change semantic kinds or identities.

### mant-sources (next release)

- Reject non-portable archive member components before host path assembly,
  including drive-relative prefixes, alternate streams, reserved Windows
  devices and trailing dots/spaces. This intentionally tightens archive
  acceptance on Unix too; native configuration path syntax is unchanged.
- Reject FIFO and other non-regular configuration/metadata inputs without
  blocking source discovery, doctor or updates. Preserve user configuration
  symlinks while rejecting managed metadata links at the Unix open boundary.

### mant-ir 0.11.0

- Restrict ordinary-list starts to `ListKind::Ordered { start: Option<u64> }`.
  JSON uses a tagged kind object and rejects the old string kind/outer start,
  or any start on bullet/plain lists. Unknown, zero, non-one and u64::MAX
  starts retain their semantics; renderers and excerpts share saturating ordinals.
- Move definition-item `inline_term` and `spacing_before_lines` into
  `layout: DefinitionLayout` (JSON `layout`). Preserve inherited versus explicit
  zero spacing and existing inline/hanging geometry. Old top-level fields and
  null layout are rejected; Rust consumers initialize `layout` explicitly.
- Converge both item shapes on `entry: Option<EntryFacts>`, with structured
  `EntryKind`, `NameCase` and derived `names`. Remove `DefinitionIdentity`,
  `DefinitionRole`, `DefinitionCase` and semantic `aliases`; migrate Rust
  consumers and unreleased v0.11 JSON together. Old/unknown fields are rejected,
  not ignored. Explicit `aliasGroups`, `aliasOf` and fragment aliases are unchanged.
- Require explicit form references for both content-owner kinds; empty forms
  mean unknown and never fall back to native terms. Invalid forms or names do
  not erase owners/children. Every selectable name needs an exact binding;
  rejected names/aliases are omitted from derived projections while independent
  valid Form, identity and literal evidence remain usable. Complete term forms
  borrow content rather than clone it.
- Add optional original-item `source` to both `ListItem` and `DefinitionItem`;
  Rust literals must initialize it. Native excerpts now report the item location,
  not the containing list. Unannotated definitions transparently expose nested
  semantic entries, matching ordinary-list ownership.

- Separate documented-name occurrences from explicit `aliasGroups` and
  same-document `aliasOf` facts. Validate visible-head binding, group disjointness,
  exact member identity, unique compatible targets and cycles without merging
  or rewriting content. Markdown declarations, semantic indexes, outlines and
  independent explanation consumers now preserve these distinct relationships.

- Introduce common `EntryFacts` for native definitions and ordinary list items,
  borrowed `EntryOwner` views, and validated owner-relative form references.
  `ListItem` gains an optional `entry`; Rust struct literals must initialize it.
  Both content owners use the same facts type with explicit `forms`; no legacy
  type alias remains. Ordinary content remains authoritative and is not
  reconstructed from these references. This deliberately revises the unreleased
  v0.11 document schema; Markdown production and all query consumers support
  both content-owner kinds.

- Add sparse `TableGrid` coordinates and bounded slot expansion for shared
  table presentation without changing the serialized table shape.

- Share direct semantic-child ownership between indexing and choice-domain
  validation. Explicit choices require nonempty value children; invalid
  producer claims make semantic projections incomplete.
- Separate normalized document-local `NodeId` values from exact
  source-authored fragment aliases on document roots, sections, and inline
  anchors. `DocumentIndex` resolves either spelling without admitting
  noncanonical identities or guessing between ambiguous fragments.
- Retain an inline anchor's addressable-owner source span independently from
  its nested IR placement, allowing target-conservation tools to distinguish
  neighbouring paragraphs, definitions, list items, and table cells.
- Make one semantic entry ID its authoritative local content address, replace
  the redundant target vector with explicit linked-document relationships,
  and attach source-declared cross-document value domains to definitions.
- Restrict semantic document references to Markdown and manual destinations,
  centralize namespace-only resolution, and remove the unused union domain.
- Validate cross-document entry domains in the shared IR, including non-empty
  references, native manual-section grammar, role sets, duplicate roles, and
  optional source provenance used to retain relationship order.
- Publish a shared classification for IR identity and relationship diagnostics
  that make semantic projections incomplete.

### mant-protocol 0.11.0

- Classify explanation owners as direct-entry, related-entry, entry-mention
  or context-mention before global paging. Add fixed four-class `counts`,
  `order: class-then-source`, Unicode match previews and omission flags.
  Scope uses one flat evidence page with `documentIndex` references into BFS
  source reports; remove nested per-document query/body/cursor copies. Global
  ordinals now follow class, document, then IR order, not source order alone.

- Introduce `mant.explanation/v0.11` and `ExplanationOptions`. Explain requests
  return independent name/form/literal/explicit-relationship evidence, not a
  unique excerpt. Scope results move from `matches`/`missed` to
  `result.explanation` with global paging, per-source reports, outcome,
  coverage and independent truncation. Consumers must inspect `outcome`, not
  assume one record or infer no-evidence from an empty later page.
- Expose explicit `aliasGroups` and `aliasOf` separately from selectable
  outline names. Update the authorized unreleased v0.11 snapshot in place;
  unrelated native/source/doctor contracts and published snapshots are unchanged.

- Change the unreleased excerpt `document-entry.entry` to a single-item IR
  block, preserving ordinary list content, numbering and layout instead of
  requiring every owner to be a standalone native definition.

- Carry document-wide `semanticsComplete` and the logical source `address` in
  excerpts and independent explanations; compact content retains an
  incomplete-semantics notice even when parser findings are hidden.
- Advance the native process family to v0.11 and expose optional
  `fragmentAliases` beside normalized document, section, and anchor identities.
  The published v0.10 schema snapshot remains immutable.
- Finalize the unreleased outline contract with a top-level logical `address`,
  typed `documentTargets`, protocol-owned resolved entry domains, and the
  broader `semanticsComplete` signal. Remove the redundant entry `targets`.

### mant-engine 0.11.0

- Recognize singular COMMAND/SUBCOMMAND headings like their plural forms,
  using complete heading words and preserving option/environment precedence.
  Affected native entries intentionally become commands with documented names
  and command-derived IDs; this is a semantic correction, not a color change.
- Keep a nonempty form/identity fallback in search entry titles without
  inventing selectable names. Search and strict navigation share the compact
  label policy; outlines explicitly prefer complete forms.
- Preserve structural tables/lists through nested mdoc enclosures and font
  scopes, and carry font and spacing state out of every structural consumer.
  Literal source lines follow macro descendants; ordinary filled text keeps
  word boundaries under `Sm off` while macro operands still obey that mode.
- Execute mdoc link labels before URI font controls and preserve the distinct
  scopes of `Lk`, multi-address `Mt`, `In` and `Xr`. Compact links retain typed
  targets even when the URI is not repeated as visible text. Function commas
  now use the next logical operand, respecting authored punctuation and
  intervening prose without replaying controls.
- Accept table recovery output, formatter state and diagnostics atomically;
  rejected candidates cannot leak state, while successful control-only cells
  still apply their effects. These lowering corrections, including the filled
  text and literal-line boundary changes above, do not change the wire schema.
- Preserve generated function/reference font and spacing effects, including
  nested `Sm`, and keep ordinary `Fo` declarations in their surrounding prose.
  SYNOPSIS declarations retain their independent boundaries. Empty `Eo`/`Ec`
  scopes consume word events and do not leak internal joins to following text.
- Carry scoped fonts into bibliographies and table recovery, keeping generated
  author conjunctions and rolling back rejected recovery state. Preserve font
  changes across physical lines inside man `EX`/`SY`, resetting at scope exit.
- Decode numbered glyphs in mandoc's 8-bit terminal range, with terminal-safe
  control filtering; unsupported or malformed indices remain visibly escaped
  instead of disappearing or being misinterpreted as arbitrary Unicode.
- Preserve tables, list terms, every column cell and navigation targets inside
  literal/unfilled displays, including nested font scopes. No-fill line layout
  no longer flattens structural payloads into an incomplete inline sequence.
- Treat zero-width text as a word event, separately from anchors and hidden
  nodes, so consumed no-space controls cannot fabricate joined option names.
  Preserve real inter-word boundaries without exporting trailing padding.

- Preserve decoded literal font spellings such as `\efB` in all styles;
  never reinterpret visible text as another round of roff controls. A real
  `groff_man_style(7)` fixture guards the complete four-font definition term.

- Model mdoc font-selecting macros as effective font scopes, not additive
  styles: local escapes override the macro default, and scope exit restores
  the outer current font without rolling back mandoc's previous-font register.
  Keep plain text and transparent macros unscoped; carry `Bf` defaults through
  nested lists and displays without adding paragraph breaks. Preserve the
  separate man font policy; test IR styles as well as Markdown.

- Bound generated `Fl` prefix joins to their operands, preserving the space
  before external arguments after empty or zero-width operands while retaining
  explicit `Ns`/`Pf` controls and zero-width targets. Preserve the distinct
  operand-less form's join to the next non-text sibling on the same line.

- Preserve mdoc inline boundary effects across prose and styled siblings:
  apostrophes attach on both sides, generated enclosure closers consume internal
  joins, and `Ns`/`Pf` honor native source-line and successor conditions.
  Definition forms, literal displays and supported tbl recovery share the same
  output-order composition without changing stored IR or schema contracts.

- Match Windows `MANCONFIG` wildcard components without ASCII case
  distinctions, including configuration filenames and wildcard directories.
  Preserve actual path spelling, shared scan limits and fragment deduplication;
  Unix wildcard matching remains case-sensitive.

- Bind recognized names from the grammar's original byte ranges, including
  Vim-style `-w{number}`, enclosing punctuation, leading whitespace and
  cross-style `[-+]O`. Native and inferred Markdown definitions retain their
  complete forms and direct Name explanation evidence without broadening
  explicit Markdown declaration or marker admission rules.
- Preserve the original head source when hanging paragraphs become definitions
  and when man `.TP/.TQ` or mdoc `.It` heads share a body. Explain and search
  report the first head rather than a later surviving head or a missing source.

- Preserve nearest semantic ownership when table cells flatten into portable
  Markdown. Compose byte ranges alongside rendered content, instead of deriving
  item boundaries from HTML anchor markers; text and Markdown presentation stay
  unchanged, including nested ordinary/definition owners and sparse tables.

- Strict binding exposed native option-name truncation at internal `+` signs.
  Preserve complete spellings such as `-nostdinc++` and `-Wc++11-compat`;
  previously truncated selectors no longer address those definitions. IDs for
  corrected names and their former collision groups must be rediscovered.
- Bind already recognized native names immediately before angle-delimited
  arguments, including Clang's `-D<NAME>` and `-fno-builtin-<function>` forms.
  Keep complete authored forms and reject invalid bindings independently.
- Reuse validated names within immutable selection snapshots and skip repeated
  name validation when there are no explicit alias groups to project. This does
  not bypass validation of names or nonempty relationships.

- Propagate bounded BSD/mandoc configuration expansion findings through native
  root discovery to doctor, including macOS fragments and Linux mandoc fallback.
  Truncated patterns no longer silently omit roots without an inspection finding.

- Share Markdown metadata constraints between import and semantic export.
  Valid public IR exceeding alias-group, member, ID or JSON payload limits
  falls back to ordinary Markdown for the whole document, rather than emitting
  annotations that the reader would reject.

- Recognize literal mentions beside typographic Unicode quotes and CJK
  punctuation, including unspaced prose. Keep executable punctuation and
  Unicode name continuations from turning longer names into prefix evidence.

- Keep executable punctuation in literal matching: `-#` no longer matches
  `-###`, nor `--` the longer `--%` marker; sentence boundaries and parameter
  forms remain usable evidence. Semantic Markdown export now proves one
  infer/fixed attached-value policy for the whole list, preserving fixed names
  and relationships or falling back to ordinary Markdown for the document.
- Retain bounded borrowed explanation plans before globally selecting a page;
  later direct/related entries can displace lower-priority mentions at the
  candidate cap. Preserve real matched-block coordinates and complete-query
  preview windows. Compact rendering distinguishes definitions from mentions
  without rewriting IR or including unrelated full mention bodies. Add
  `resolve_explanation_block` for snapshot-local preview coordinates.

- Introduce an independent bounded `explain_query` evidence collector and
  `mant.explanation/v0.11` result: preserve independent same-name owners,
  distinguish names/forms/literal support/explicit relations, and report
  semantic result paging separately from body and relationship bounds.

- Add opt-in semantic Markdown export for homogeneous explicitly declared
  ordinary-list owners, including IDs, explicit aliases and value domains.
  Unsupported documents retain portable content without claiming a lossless
  semantic round trip; IR JSON remains the complete serialization.

- Accept bounded, annotation-only `mant:entry` JSON for explicit IDs,
  visible-name alias groups and same-document alias relationships. Validate
  forward references, subject/case compatibility and cycles through the shared
  IR policy; reject competing objects atomically and invalid relationship fields
  independently while keeping original content and successful sibling entries.

- Separate inline definition first-paragraph hanging layout from standalone
  description blocks. Continuations now retain their structural indentation
  in text and the TUI regardless of label width; explicit leading and
  inter-paragraph spacing survives the join. No IR wire fields were added.

- Recognize local links to ordinary list-item entries as valid navigation
  targets without inserting redundant anchors into their visible content.
  Missing IDs, duplicate identities and role collisions still report errors.

- Reject local exhaustive-choice claims after partial child extraction or a
  failed child-list declaration, while preserving successful siblings and
  visible content. Failures propagate through ordinary nested bullet and
  ordered containers to their nearest semantic owner, without leaking past
  recognized child entries or across unrelated document content. Explicit and
  inferred open choices remain non-exhaustive.
  Bind declarations to original parser item positions, not the first retained
  content block, so removing leading comments cannot lose failure or domain ownership.

- Keep indented definition continuations together across explicit vertical
  spacing, including GCC help classes, qualifiers and trailing examples.
  Bound lookahead at the next substantive block; do not absorb outer content
  or trailing spacing. Hanging definitions share the same boundary. This also
  removes Clang's spurious detached `-O4 and higher` entry while retaining the
  real `-O4` alias and its complete optimization-level explanation.

- Align plain-text non-inline definition indentation with the IR/TUI's
  four-column description origin (previously two columns in plain text).
  This prevents a continuation's layout from drifting when its semantic
  ownership is restored. TUI indentation is unchanged.

- Preserve ordinary Markdown list items while adding semantic facts, with
  final-IR name/form bindings instead of deleting head delimiters. Explicit
  role/case declarations support ordered lists and reject invalid items
  independently. Nested content, numbering, tightness and links remain intact.

- Share entry coordinates and original-content excerpts across ordinary list
  items and native definitions. Read, outline, search, scope traversal and TUI
  navigation now support both, including explicit Markdown authoring.

- Retain every visible name inside linked native definition heads and stop
  name extraction at adjacent styled parameters (`-L` plus italic `dir`).
  Preserve complete displayed forms and fixed names such as `-Wall` / `-O2`;
  a bold invocation fragment no longer becomes a malformed command name.

- Retain historical `-h/--help` option aliases across Markdown and native
  manuals without splitting argument paths or assignment values into names.
  Slash candidates retain inline styling until parameter exclusion, including
  adjacent mdoc `Ar` and man alternating-font arguments.
- Ignore empty styling wrappers when locating an option invocation, so
  emphasized metavariables after alias punctuation cannot become selectors.
- Preserve continuation and pending spacing across nested no-fill styling
  containers by sharing the source-line cursor and inline formatter state.
  Explicit breaks and spacing requests remain visible to that state while
  entering a font scope, avoiding an extra blank row at styling boundaries.
- Extract option and styled command names per alias group without treating
  argument tokens or environment-assignment values as additional aliases.
  Command context follows semantic ancestry, not visual indentation; malformed
  variable subscripts remain visible with an unclassified-entry diagnostic.
- Resolve decoded Markdown fragments exactly, without stripping another `#`,
  trimming authored whitespace, changing case, or guessing a normalized ID.
- Preserve literal display breaks, spacing controls, font state and explicit
  continuations; ordinary man paragraphs reset the prevailing tag width.
  Distinguish mdoc typewriter `Qq`/`Qo` from typographic `Dq`/`Do` quotes.
- Derive Markdown list tightness from direct parser-item structure, independently
  of nested list spacing. LF, CRLF, CR and space/tab blank lines share source
  location and paragraph-spacing rules without changing original byte offsets.
- Keep literal HTML anchors searchable in code and use parsed HTML/item
  ranges for internal source-map markers and semantic search ownership.
- Consume Markdown semantic directives in the original parser event tree;
  removing a directive cannot merge independent lists or spread a role/domain.

- Retain separate native table boundaries and logical columns after spans;
  text and Markdown no longer shift the following cell left.

- Preserve the complete visible glyph after `\z`, including named glyphs,
  while approximating its zero advance in renderer-neutral text.
- Share stateful man font decoding across prose and table recovery: explicit
  resets override macro defaults, consecutive text keeps its font, and `fP`
  restores the previous selection. Retain `.ft` and `.SM` font behavior.
- Preserve empty mdoc enclosures and authored `Eo` delimiter placement by
  visiting structural parts exactly once, including table recovery.
- Lower every mdoc `Mt` operand as its own email link, retaining multiple
  addresses and trailing punctuation in prose and recovered table cells.
- Preserve one function argument per `Fo`/`Fa` operand; quoting defines
  multi-word parameters, and controls/targets no longer consume argument slots.
- Make tbl cell recovery transactional: unsupported mixed block/inline
  requests keep the complete native payload or complete source spelling,
  never a partial reconstruction that silently drops earlier text.

- Decode local Markdown path/fragment escapes once before validation and
  re-encode logical destinations when rendering Markdown, preserving spaces,
  Unicode, literal percent escapes, and registered-source confinement.

- Classify URI schemes and authorities before relative Markdown destinations,
  so external `.md` URLs cannot create local document links or scope edges.

- Render mdoc `.In` as a header reference in prose and as an include directive
  only at a synopsis source-line start, including recovered table cells.

- Preserve the optional brackets and option/metavariable styling of man `.OP`
  requests inside and outside command synopses.

- Reuse dialect-specific native inline parsing for supported tbl text blocks,
  preserving nested calls, explicit enclosure closers, links, and cross-line
  spacing state instead of reconstructing a separate flat macro language.

- Preserve validated argument-less `.Tg` destinations and original fragment
  spellings when recovering explicit targets on empty mdoc list items.
- Carry pending list targets' native owner source spans through attachment,
  rather than attributing recovered targets to a later item or table row.

- Bound cumulative native indentation and report excessive or unsupported
  offsets instead of overflowing; display offsets now share horizontal unit
  conversion with definition widths.
- Reject special files at the shared manual-path configuration boundary before
  and after opening. Unix nonblocking opens prevent FIFO fragments, including
  macOS `MANCONFIG` matches, from hanging discovery while preserving regular
  file symlinks and existing read budgets.
- Accept local `mant:domain choices=exhaustive|open` declarations with structural
  validation. Reject mixed domain modes and incompatible child kinds while
  keeping entries and their visible descriptions addressable.
- Let Markdown authors separate invocation forms with `|` between code terms
  while retaining comma-grouped aliases within each form. Both forms share one
  entry identity, and empty alternatives reject the declaration atomically.
- Escape adjacent plain-text runs as one unit so AST segmentation cannot
  introduce redundant intraword escapes. Preserve delimiter protection at
  actual style boundaries and clarify raw-Markdown versus visible search.
- Deduplicate unresolved scope links per origin and cache failed qualified
  resolutions within a request, without hiding distinct referring documents.
- Preserve argument-less `.Tg` targets that remain on their own native owner,
  including `Va`, `Pa`, and `Ar`, and keep exact leading hyphens in authored
  destinations. Only retained native targets reserve the section namespace.
- Bound manual configuration work across directory enumeration, wildcard
  matching, path components, and fragments, not just successful matches.
  Windows configuration reads share byte and line limits; root directives
  now consistently denote literal directories while `MANCONFIG` expands globs.

- Recognize semantic-entry directives at arbitrary CommonMark list depth and
  preserve complete non-option names rather than splitting punctuation inside
  a code span.
- Accept a document link wrapping one code term as an entry destination, and
  add item-scoped `mant:domain` declarations for evidence-backed value spaces
  in relative Markdown documents or exact native manuals.
- Include linked entry destinations and entry-set domains in bounded document
  traversal in authored source order, and show both relationships in expanded
  compact outlines. Plain-text CLI outlines now share that renderer, preserve
  resolved Markdown fragments, and distinguish labels from destinations.
- Attach `mant:domain` to the structural CommonMark list item rather than a
  coincidental source line, including multiline, nested, blank-line, and CRLF
  forms. Require exact directive names and reject duplicate roles or malformed
  manual references without reporting a complete semantic projection.
- Treat repeated `mant:domain` declarations on one entry as ambiguous: retain
  the diagnostic but attach and traverse none of the competing relationships.

### libmandoc-rs 0.10.0

- Retain the first data row of each native table in `NodeFlags::table_start`,
  independently of leading rules and `T&` layout changes.

- Expose effective table-cell content kinds, including layout-rule precedence
  and connecting versus isolated horizontal rules, for faithful consumers.

- Reject excessive native syntax nesting before validation and syntax/equation
  nesting before reference rendering; release both trees iteratively so depth
  errors and truncated ownership transfers remain safe to clean up.
- Close incomplete root font-macro scopes at EOF without dispatching the
  synthetic root token as a macro; malformed `.I`, `.B`, `.R`, `.SM`, and
  `.SB` inputs now report recoverable findings instead of aborting the process.
- Avoid an upstream assertion when an explicit mdoc `.Tg` precedes an already
  tagged section by preserving the existing section target instead of trying
  to assign it twice.
- Preserve the authored `-tag`, `-diag`, `-hang`, `-inset`, or `-ohang`
  subtype of normalized mdoc definition lists so semantic consumers do not
  need to recover source syntax after parsing.
- Treat this as a breaking pre-1.0 release because the preserved subtype adds
  a field to the public owned `Node` structure; downstream structure literals
  must initialize `definition_list_style`.

### mant-engine 0.11.0

- Discover `%APPDATA%\ManT\man` as the primary automatic Windows native-manual
  root after configured `man.conf` entries and before the
  `%USERPROFILE%\.local\share\man` compatibility fallback. Explicit
  `MANT_MANPATH` and `MANPATH` behavior is unchanged.
- Extend the ManT-owned Windows `man.conf` with bounded, one-level
  `MANCONFIG` fragments, PATH-conditioned `MANPATH_MAP` entries, trailing
  `MANDATORY_MANPATH` roots, optional double-quoted paths, and case-insensitive
  single-pass `%NAME%` expansion. Invalid directives are omitted and exposed
  through the new `inspect_manual_roots` diagnostics used by `mant --doctor`.
- Match process-environment names with Windows' case-insensitive semantics,
  deduplicate final roots by Windows path equivalence, stop traversing
  `MANCONFIG` patterns once the 256-fragment budget is exhausted, and diagnose
  both that truncation and known directives with missing arguments.
- Separate Windows `MANCONFIG` resource limits into at most 4096 matching path
  candidates traversed and at most 256 unique configuration fragments loaded,
  so overlapping patterns neither waste fragment capacity nor bypass the scan
  bound.
- Keep target-only native definition containers as zero-width navigation
  placements without manufacturing empty `term-entry-*` semantic entries.
- Retain formatter-generated native anchors independently of semantic-entry
  discovery, including function destinations whose normalized spelling
  collides with a surrounding section identity.
- Preserve exact Markdown heading IDs and explicit mdoc `.Tg` spellings as
  fragment aliases while assigning every destination a normalized internal
  identity. Addressable Markdown emits both forms and local references continue
  to resolve to the canonical target.
- Retain explicit `.Tg` destinations before empty mdoc bullet, ordered, plain,
  column, and definition items, including target-only list bodies, instead of
  discarding the target when the item has no visible descendant.

- Reconstruct source-proven man(7) `.IP` and `.TP` enumerations as ordered
  lists from the first punctuated item, keep sibling `.RS` continuations in
  that item, and stop exposing singleton footnotes or numbered steps as
  semantic entries while preserving bare numeric value domains such as
  `0`/`1`.
- Recover complete consecutive numbered procedures authored as mdoc
  `Bl -tag` lists while retaining singleton, gapped, mixed-style, bare, and
  decimal terms as definitions.
- Preserve validated mdoc targets moved by libmandoc onto paragraph, display,
  list, item, function, and section owners; keep `.Tg` zero-width; and run the
  same navigation-resolution passes over root and section content. Explicit
  `.Tg` spellings and automatic wrapper fallbacks now come from source tokens,
  so inherited parser tags and renderer spacing cannot change a destination.
- Add an independent target-conservation audit and a licensed `libpipeline(3)`
  regression fixture so invisible destination loss is covered separately from
  visible-content, structure, CommonMark, and renderer-layout audits.
- Upgrade target conservation to an exhaustive v2 owner policy: every native
  deep-link owner is retained, deliberately excluded with a reason, or fails
  review as unclassified. Compare one shared parse on both sides, audit exact
  fragment aliases and normalized identities, and report unexpected targets,
  role collisions, invalid identities, duplicates, and dangling links as
  distinct signals.
- Upgrade target conservation to an occurrence-aware v3 contract that binds
  every logical AST owner to a compatible IR role, section, and container;
  same-named unrelated identities and arbitrary hexadecimal suffixes no longer
  produce false-clean audit results.
- Upgrade target conservation to a position-aware v4 contract that matches
  each obligation to the source line of its concrete addressable owner and
  reports targets moved onto same-kind sibling structures as both missing and
  unexpected.
- Add a separate semantic-entry precision audit for ordinal definitions,
  empty entries, and invalid value-domain children, while retaining aliasless
  generic and note-section entries as reviewable census signals rather than
  language-specific rejection rules.
- Upgrade semantic precision to a v2 source-side conversion ledger. Every mdoc
  ordinal candidate records its authored definition-list subtype and term
  sequence, and its expected retained/recovered disposition must match the IR
  block at the same source line, exposing over-conversion even after the
  original term has disappeared.

### mant-ui 0.11.0

- Defer POSIX termination until pager setup completes, restore raw mode and the
  alternate screen, then terminate with the original signal. Early and active
  paging interruptions share the same restoration guarantee as the reader.

- Preserve table-cell anchors and exact fragment aliases through cell wrapping,
  narrow-view stacking and nested tables; destinations resolve to their own
  content row rather than disappearing or falling back to the table start.
- Share logical span-aware column placement with CLI output.

- Apply the same Unicode scalar lowercase transform to search queries and
  rendered text, preventing identical Greek sigma text from being missed
  while retaining exact cell highlights through expansions and wrapping.

- Resolve exact document, section, and inline fragment aliases to the same TUI
  rows as their normalized internal targets.
- Emphasize literal commands and options in tldr examples while leaving
  replaceable placeholders in the ordinary foreground color, matching their
  source semantics in both the TUI and one-shot terminal presentation.

### mant 0.11.0

- Migrate CLI/request JSON/MCP explain to bounded multi-evidence results.
  Readable zero/multiple results now exit 0; strict `--node` and `mant_read`
  retain ambiguity errors. `--limit`/`--offset` page owners globally and
  `--explain-content-bytes` bounds payload copies. MCP exposes equivalent
  `maxResults`/`offset`/`contentBytes`, independently of Unicode character paging.
  Update help, schema discovery, protocol description and the independent clap
  self-manual oracle; real names and declared alias groups are checked separately.
- Make all public CLI options addressable in the self manual, including
  document-scope, input, discovery, doctor, and dry-run options previously
  described only in prose or tables. Keep their examples and constraints in
  the semantic definition returned by explanation and outline queries.
- Replace the independent help examples with the self-manual's generated,
  embedded TLDR. Both help and no-argument usage end with the quick reference
  followed by optional self-manual commands, styled consistently with help.
  No installed documentation is required to display the examples.
- Replace `--ui` and `--no-pager` with the independent
  `--display auto|direct|pager|tui` policy. Migrate explicit reader invocations to
  `--display tui`, and noninteractive invocations to `--display direct`.
  Automatic terminal text queries now share the less-like pager with catalog
  output; short content prints directly, while stdin requests and machine
  outputs never automatically page. Explicit interactive modes require a
  usable terminal, and format/colour rules remain independent of delivery.
- Point help and missing-action diagnostics to `mant mant` for full reading and
  `mant mant --outline` for focused exploration, without automatically loading
  documentation or changing the TUI.
- Default all CLI document queries to text, including redirected full reading,
  stdin documents, and request JSON. Use `--format markdown` for the former full
  output default; `--preserve-anchors` still selects Markdown explicitly.
  Share full/excerpt text colour presentation without changing visible layout.
  Automatic TUI reading, MCP presentations, and JSON maintenance reports remain
  unchanged.
- Advertise and accept the v0.11 native protocol family required by the new
  fragment-alias wire model.

## 0.10.0 - 2026-08-31

### mant-ir 0.10.0

- Add a rebuildable `SemanticIndex` that keeps content definitions independent
  from semantic discovery, preserves exact aliases separately from authored
  forms, reconstructs nested command/parameter/value ownership, and exposes
  compact `EntrySummary` coverage.
- Expand entry semantics to distinguish commands; option, marker, and operand
  parameters; configuration keys; environment variables; variables; values;
  and generic terms. Add evidence-backed value domains without heuristically
  inferring cross-document sets from prose.
- Extend `OutlinePath` with nested entry coordinates such as `2.3/e4/e2`.
- Centralize external-URI and conservative email-address validation, including
  RFC 3986 ASCII registered names, root dots, percent-encoded hosts, HTTP
  authority, IPv6 and port structure, percent-decoded single-recipient mailto
  validation, and inverse percent-encoding for typed email targets.
  Validate source spans attached to producer diagnostics as well as document
  content.

### mant-protocol 0.10.0

- Advance the complete native protocol family from v0.9 to v0.10. This is a
  breaking wire change; v0.9 clients must regenerate schemas and send the new
  discriminators.
- Replace outline `detail` with the tagged `entries` projection (`none`,
  `summary`, `all`, or selected kinds) and optional `root`. The default summary
  reports semantic coverage without materializing every entry.
- Expand outline entry nodes with typed kinds, exact aliases, authored forms,
  content targets, optional value domains, summaries, and nested children.
  Exact paths and IDs win before aliases, and ambiguous aliases return stable
  candidates.
- Define kind-filtered outlines as matching entries plus their structural
  ancestors, with filtered summaries and an explicitly empty node set when no
  selected kind exists.
- Give scoped search one authoritative global pagination contract. Per-document
  groups now carry only their canonical Markdown render descriptor and globally
  numbered hits; nested local offsets, continuation cursors, and totals are
  removed from the unreleased v0.10 wire shape.
- Version explicit tldr maintenance output as `mant.tldr-update/v1` and expose
  its independent generated schema without adding mutation to MCP.
- Publish the shared 16-selector maximum and enforce it at every native,
  in-process, and MCP excerpt boundary rather than relying on schema metadata.
- Include doctor and tldr-update in `--schema all` while retaining their
  independent discriminator families, and require every published search
  occurrence to contain at least one presented line range.

### mant-engine 0.10.0

- Build every outline from the source-neutral semantic index. Native man/mdoc
  definitions now retain grouped forms, including alternative mdoc terms, and
  nested option/value hierarchies without duplicating content definitions.
- Add `build_outline_projection` for summary, full, role-filtered, and rooted
  discovery while retaining `build_outline_with_detail` as an in-process
  compatibility convenience.
- Give outline, excerpt lookup, explanation, addressable Markdown, and search
  ownership one nested entry-coordinate topology. Every projected term or
  nested entry path now round-trips through focused reads.
- Recognize multi-item key-binding command groups under topical headings and
  preserve complete hyphenated Readline command names as exact aliases.
- Recognize complete hyphenated Readline variable names inside variable
  sections. Their first segments are never aliases or IDs, so names such as
  `bind-tty-special-chars` cannot silently shadow the `bind` builtin.
- Separate inferred native semantic IDs from formatter navigation anchors and
  derive role-qualified IDs from complete recognized names, preventing short
  anchors such as `set` or `re` from shadowing unrelated exact aliases.
- Normalize formatter-generated man and mdoc anchors through the semantic ID
  slug rule, allocate repeated tags uniquely against section identities, and
  reserve all resulting anchors before assigning semantic entry IDs.
- Make generated entry collision IDs content-addressed and native section IDs
  independent of unrelated siblings, so reordering cannot silently redirect a
  returned ID. Paths remain explicit source-order coordinates.
- Use one selector resolver for excerpts, rooted outlines, and explanations:
  exact path, exact ID, exact alias, then shorthand. Explanation now rejects a
  resolved structural node instead of bypassing it for a lower-precedence
  same-spelled entry.
- Apply one context-bounded environment-variable grammar to native and
  Markdown definitions, including shell, PowerShell provider, Windows percent,
  and single-assignment forms. Complete terms and explicit trailing default
  annotations are parsed without first-word truncation; unresolved native
  definitions remain visible with a structured incompleteness diagnostic
  rather than promoting prose.
- Prune unrelated topology and recalculate summaries for selected-kind outline
  projections; a zero-match projection now renders an explicit empty result.
- Rebase scoped-search hit ordinals across breadth-first document order so one
  response never contains duplicate line-group numbers.
- Index semantic aliases and outline IDs once while producing discovery
  diagnostics, avoiding quadratic selector scans on definition-heavy manuals.
- Let explicit Markdown declarations produce every v0.10 semantic role and
  preserve strictly formed negated dash options such as
  `!--reloadEnvironment` without treating arbitrary `!name` tokens as options.
- Reserve explicit mdoc `.Tg` targets before allocating section IDs, and keep
  `.PD 0` as layout rather than using it to merge independent `.TP` entries.
- Preserve compact man `.TP` alias groups only across verified option-shaped
  terms, stopping at prose, indentation, and section boundaries so aliases
  share their description without merging unrelated definitions.
- Keep `.TP`, `.TQ`, and compact `.IP` alias groups on exact source-backed
  boundaries, preserve their authored order with a linear merge, and diagnose
  uncertain descriptions instead of absorbing unrelated pending heads.
- Clamp search `lineRanges` to the exact trailing-space-trimmed UTF-8 lines
  published in previews and context so structured coordinates never point
  outside their presented text.
- Bound compiled regex programs and DFA caches, omit matches that exist only
  in source-map anchors or synthetic separators, and keep structured matched
  text and ranges on the anchor-free presentation surface.
- Enforce selector length and excerpt-count bounds inside the projection engine
  so preloaded and standard-input producers cannot bypass request validation.
- Classify mailto recipients only after shared decoding and structural
  validation, serialize typed email targets through the inverse shared encoder,
  and keep invalid targets visible but inert in deterministic Markdown.
- Require `mant-sources ^0.9.2` so the source-health guarantees cannot resolve
  to an older compatible patch through an existing lockfile.

### mant-ui 0.10.0

- Align the interactive Outline with protocol discovery: collapsed entry
  groups display direct, nested, and authored-form counts, while expansion
  reveals the complete role-aware hierarchy and multi-form labels.
- Keep the default Outline dense by labeling semantic entries with exact
  aliases instead of parameter-heavy authored forms. The selected entry still
  expands to its complete form, and **View → Full Outline Labels** can wrap all
  visible labels for review.
- Anchor the selected Outline node to its current viewport row across full-label
  changes, whole-tree expansion or collapse, and sidebar-width reflow, moving
  it only when terminal bounds or complete-title visibility require it.
- Keep dismissed search highlights hidden across redraws and resizing while
  retaining the confirmed query for later navigation.
- Represent host-activatable external links with a validated `ExternalUri` so
  Markdown and tldr producers share the same HTTP(S)/mailto policy. Embedding
  callbacks now receive `&ExternalUri` instead of an unclassified `&str`;
  rejected schemes remain visible but inert.
- Reuse the IR's structural URI validator at that activation boundary, rejecting
  malformed percent escapes, userinfo, empty ports, IPv6 authorities, and
  mailto dot-atoms while retaining valid query-bearing mailto actions.
- Reserve a one-column Outline scrollbar gutter only when its narrow layout
  actually overflows, preserving final label cells without wasting sidebar
  width in non-scrolling outlines.
- Restore raw mode and the alternate screen from the normal event loop before
  re-raising POSIX termination signals, while a second signal retains its
  immediate default behavior.

### mant 0.10.0

- Make `--outline` return section topology plus semantic summaries by default.
  Use `--outline-entries none|summary|all|KINDS` to control expansion and
  `--outline-root SELECTOR` to focus one section or entry. The former
  `--outline=entries|sections|options` syntax is removed.
- Give `mant_outline` the same `entries` and `root` inputs as native request
  JSON, keeping MCP, CLI, and the TUI on one projection model.
- Guide agents through stateless summary → rooted expansion → focused read
  calls while preserving returned paths and stable IDs across each step.
- Expose catalog regex/case/result offsets and search representation/global
  offsets through MCP. Scoped search presentation now reports one unambiguous
  global matching-line-group total and never emits a document-local CLI cursor.
- Preserve the bounded prose-only explain probe in scoped CLI and MCP results,
  including its document, outline node, and line as a qualified failure; truly
  absent selectors remain sparse misses.
- Present empty kind-filtered outlines consistently in terminal, deterministic
  text, CommonMark, and MCP output instead of returning the full section tree.
- Give completely missed scoped explanations an executable next step: native
  output names the complete outline command, while MCP names `mant_outline`
  and `mant_search` without leaking CLI-only flags.
- Apply the same 512-Unicode-scalar, control-free selector validation and usage exit
  status to single-document and multi-document outline/read/explain requests.
- Sanitize and bound every MCP error through the same presentation boundary as
  successful pages, without exposing physical registry paths or panic details.
- Launch native Windows external links through the absolute System32 handler
  path instead of executable name lookup, and sanitize dynamic TUI notices at
  their final terminal-rendering boundary.
- Require `mant-sources ^0.9.2` for maintenance commands so an existing
  lockfile cannot retain the pre-health-check implementation.
- Describe healthy installed sources as locally consistent in offline doctor
  reports and state explicitly that remote freshness was not checked.
- Reject oversized or control-bearing standard-input node selectors through the
  same engine-owned validation used by file, logical-document, and JSON inputs.
- Keep multiline regex parser diagnostics readable through independently
  sanitized lines, and recover MCP stdio after malformed, deeply nested, or
  oversized frames while bounding framework-generated parameter errors.

### mant-sources 0.9.2

- Reject linked managed-source roots and metadata during registration.
- Compare the recorded document count with the materialized Markdown count so
  doctor reports count-mismatched caches and update reacquires the source.
- Use complete-object depth-one clones on Windows, avoiding Git for Windows
  failures while hydrating a blob-filtered no-checkout clone by pathspec.
- Bound the complete decompressed tar stream before parsing so hidden GNU
  long-name/long-link and local PAX metadata cannot allocate outside source
  acquisition budgets.
- Apply one physical managed-root gate to update, discovery, doctor, and
  metadata reads; apply the registry's logical-path normalization during
  installation; and reject invisible Unicode formatting identities.
- Accept explicit archive `./` components, but charge every tar entry before
  type dispatch so directory and metadata payloads cannot evade expanded-size
  limits.
- Parse configured logical document identities independently of host path
  rules, rejecting backslashes before Windows can reinterpret them while
  converting discovered native relative paths to portable slash-separated
  identities at the filesystem boundary.
- Validate raw ZIP names and tar member bytes as POSIX archive identities
  before constructing host paths, preserving valid slash-separated archives
  on Windows while rejecting non-portable backslash members.

See the complete [ManT 0.10.0 release notes](https://github.com/BryanHeBY/ManT/releases/tag/v0.10.0).

## 0.9.1 - 2026-08-24

### mant-ir 0.9.1

- Establish the first independently versioned patch baseline without changing
  the public Rust model or its Serde representation.

### mant-protocol 0.9.1

- Replace the exact `mant-ir =0.9.0` package edge with `mant-ir ^0.9.0`, so
  compatible IR patch releases can coexist in one resolved dependency graph.
- Retain the complete v0.9 wire shape and discriminator family; this crate
  release does not require clients to regenerate schemas or migrate requests.

### mant-sources 0.9.1

- Establish the first independently versioned patch baseline without changing
  the public registry, configuration, or update contracts.

### mant-ui 0.9.1

- Add terminal-cell-aware mouse text selection and typed host clipboard
  requests. Visual selections remain plain text, while complete addressable
  nodes can request deterministic Text or structurally complete Markdown;
  synthetic Outline groups are rejected.
- Add the `run_with_catalog_and_scope_and_copy` embedding boundary. Existing
  run functions retain their signatures and report an in-reader notice if an
  embedding host without clipboard integration invokes a copy action.
- Copy a completed mouse selection immediately, show successful copies in a
  short-lived non-modal popup, and omit presentation-only tldr panel borders
  from visual text. Size Edit menus from their complete item labels so copy
  actions remain visible and clickable.
- Continuously scroll the document while a selection drag remains at either
  vertical viewport edge, and let Shift-modified clicks or drags extend the
  retained selection by preserving its true mouse-down anchor and moving its
  active endpoint before copying it.
- Replace the upper-right document title with a clickable, bounded tab stack.
  Tabs retain first-open order and the last selected semantic node, deduplicate
  logical documents, remain transactional across host load failures, and use
  terminal-aware middle truncation plus overflow controls.

### mant-engine 0.9.1

- Prevent embedded `.so` requests from reading process-working-directory
  files when ManT parses untrusted manual content with includes denied.
- Bound raw mdoc enclosure reconstruction inside `tbl` text cells and retain
  overflow tokens as visible text instead of allowing deeply nested source to
  overflow parser or renderer stacks.
- Preserve formatter-owned commas when one mdoc `Fa` invocation supplies
  multiple parameters to a block-form function declaration.
- Keep later `tbl` text-block cells aligned when an earlier empty `T{ T}` cell
  is normalized away by libmandoc.
- Carry an outer mdoc `.Sm off` state into preformatted displays, decode the
  NetBSD `\\[vc]` named character, and retain visible digits immediately after
  signed legacy `\\s` size escapes.
- Project bounded native-tree omissions as
  `manual.syntax-depth-truncated` and `manual.equation-depth-truncated`
  diagnostics so structured consumers need not match warning prose.
- Require `libmandoc-rs ^0.9.1`, the first release containing the native parser
  fixes this engine now relies on, and publish these changes as
  `mant-engine 0.9.1`.

### libmandoc-rs 0.9.1

- Enforce `IncludePolicy::Deny` at the native file-open boundary while keeping
  the caller-selected top-level file readable; an unset include root no longer
  permits an implicit process-working-directory fallback.
- Apply a 10,000-replay aggregate budget across all roff `.while` statements
  and user-macro calls in one parse, preventing individually bounded loops from
  multiplying into process-scale memory exhaustion.
- Add compatible explicit `man`/`mdoc` input selection and bounded,
  cross-platform `SourceBundle` trees for caller-owned `.so` resolution with
  no host-filesystem fallback.
- Add a default-off `render` feature for bounded libmandoc ASCII,
  locale-independent UTF-8, and HTML reference output. Rendering never writes
  to process standard output, rejects overflow without returning a partial
  result, and does not replace ManT's existing owned-AST integration.
- Make the strict `IncludePolicy::Root` filesystem boundary available on
  Windows. The Rust resolver rejects lexical escapes and reparse points,
  verifies the opened file remains beneath the approved root, supports
  relative and gzip-compressed `.so` targets, and keeps `SourceTree` Unix-only.
- Resolve same-directory `.so` targets correctly when both the approved root
  and top-level Windows source path are relative to the process directory.
- Make Windows file and include handling match the supported Unix contract for
  explicit and fallback gzip paths, `./` components, source identity, and
  pre-epoch or permissively normalized manual dates; reject reserved devices
  and in-root reparse points explicitly.
- Preserve populated `.TP`/`.TQ` heads ending in a `\\c` continuation when a
  following tag starts, so long and short option spellings remain aliases
  instead of deleting the first tag.
- Make parsing and reference rendering independent of the caller's locale,
  and let callers pin the fallback operating-system value for a bare mdoc
  `.Os` through `Parser::with_mdoc_operating_system`.
- Normalize private libmandoc layout sentinels before exposing AST text or
  validated tags. Report syntax/equation depth truncation through typed
  `DiagnosticCode` values, and fail without writing to process stderr when
  private diagnostic capture cannot be created.
- Namespace every bundled C definition under `mant_vendored_*` so linking the
  crate no longer injects generic symbols such as `strlcpy`, `ohash_init`, or
  `mparse_alloc` into downstream binaries.
- Scope the Rust gzip decoder to Windows production builds while retaining
  cross-platform gzip fixtures as development dependencies; Unix production
  file transport continues to use libmandoc's native zlib path.
- These additive changes ship as `libmandoc-rs 0.9.1`.

### mant 0.9.1

- Sanitize dynamic newlines in single-line stderr failures so document names
  and filesystem-derived identities cannot forge `hint:`, `warning:`, or other
  diagnostic lines. Intentional multi-line tldr advice remains independently
  sanitized and styled.
- Provide native Linux, macOS, and Windows system-clipboard integration for the
  interactive reader. The clipboard is initialized lazily and retained for
  the TUI session; copy payloads are rejected above 4 MiB rather than
  truncated.
- Route clipboard writes through OSC 52 before touching a native display in
  WSL, SSH, and VS Code remote sessions, while retaining OSC 52 as the local
  fallback when native clipboard access fails. Terminal delivery is
  write-only and therefore cannot claim that the outer terminal accepted it;
  reject terminal payloads above 400 KiB before Base64 expansion rather than
  reporting success for a control string common terminals will discard.
- Require `mant-ui ^0.9.1`, the first compatible release that exposes the
  typed clipboard callback used by the executable.

### Workspace publication transition

- This publication establishes `0.9.1` for all seven crates once, because
  the four dependent package manifests must publish their internal dependency
  changes from exact `=0.9.0` requirements to explicit caret requirements,
  while every package-visible post-0.9.0 change receives a new immutable crate
  identity. The dependency roots establish the same independent-version
  baseline. This packaging transition does not by itself change public Rust
  APIs or the
  `mant.request/v0.9` and related process protocol identifiers.
- Published `mant-protocol 0.9.0` still requires exactly `mant-ir 0.9.0`; a
  consumer that also requires `mant-ir 0.9.1` can receive an unresolvable
  dependency graph rather than a second compatible-line copy. Starting with
  the first independently published `mant-protocol 0.9.1`, its `mant-ir ^0.9.0`
  requirement accepts compatible `0.9.x` releases, including `0.9.1`, but not
  `0.10.0`.
- A future dependency minimum is raised when a crate adopts a newer API. For
  example, use `^0.9.2` when `0.9.2` is the first compatible dependency, and
  publish a new version of the dependent crate with that manifest change.
  Breaking pre-1.0 changes require a new minor version and corresponding
  dependent releases.

## 0.9.0 - 2026-08-22

All seven crates were published at `0.9.0`; internal edges in that published
dependency graph use exact `=0.9.0` requirements and therefore form one
lockstep compatibility set.

### Breaking compatibility changes

- `mant-protocol`, `mant-engine`, and `mant` advanced native process contracts
  from the v0.8 family to `mant.request/v0.9`, `mant.query/v0.9`,
  `mant.document/v0.9`, `mant.catalog/v0.9`, and the related v0.9
  discriminators. Clients must regenerate schemas with `mant --schema all`;
  v0.8 requests are rejected by the 0.9 process boundary.
- `mant` changed MCP paging to stateless Unicode-scalar offsets. Clients pass
  the returned `nextChar` as `startChar` instead of retaining an opaque cursor.
- `mant_search.maxMatches` is capped at 100; `mant_find.maxResults` remains
  capped at 10,000.

See the complete [ManT 0.9.0 release notes](https://github.com/BryanHeBY/ManT/releases/tag/v0.9.0).
