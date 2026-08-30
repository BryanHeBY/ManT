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

### mant-ir 0.10.0

- Add a rebuildable `SemanticIndex` that keeps content definitions independent
  from semantic discovery, preserves exact aliases separately from authored
  forms, reconstructs nested command/parameter/value ownership, and exposes
  compact `EntrySummary` coverage.
- Expand entry semantics to distinguish commands; option, marker, and operand
  parameters; configuration keys; environment variables; variables; values;
  and generic terms. Add evidence-backed value domains without heuristically
  inferring cross-document sets from prose.
- Extend `OutlinePath` with nested entry coordinates such as `2/e3/e1`.
- Centralize conservative external-URI and email-address validation, including
  HTTP authority, host, IPv6 and port structure, and validate source spans
  attached to producer diagnostics as well as document content.

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

### mant-sources 0.9.2

- Reject linked managed-source roots and metadata during registration.
- Compare the recorded document count with the materialized Markdown count so
  doctor reports count-mismatched caches and update reacquires the source.

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
