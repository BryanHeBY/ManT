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

### mant-ui

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

### mant-engine

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
- Require `libmandoc-rs ^0.9.1`, the first release containing the native parser
  fixes this engine now relies on, and prepare these changes as
  `mant-engine 0.9.1`.

### libmandoc-rs

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
- Normalize private libmandoc layout sentinels before exposing AST text, report
  syntax/equation depth truncation explicitly, and fail without writing to
  process stderr when private diagnostic capture cannot be created.
- Namespace every bundled C definition under `mant_vendored_*` so linking the
  crate no longer injects generic symbols such as `strlcpy`, `ohash_init`, or
  `mparse_alloc` into downstream binaries.
- These additive changes are prepared as `libmandoc-rs 0.9.1`.

### mant

- Sanitize dynamic newlines in single-line stderr failures so document names
  and filesystem-derived identities cannot forge `hint:`, `warning:`, or other
  diagnostic lines. Intentional multi-line tldr advice remains independently
  sanitized and styled.
- Provide native Linux, macOS, and Windows system-clipboard integration for the
  interactive reader. The clipboard is initialized lazily and retained for
  the TUI session; copy payloads are rejected above 4 MiB rather than
  truncated.
- Require `mant-ui ^0.9.1`, the first compatible release that exposes the
  typed clipboard callback used by the executable.

### Workspace publication transition

- The next publication is prepared as `0.9.1` for all seven crates once, because
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
