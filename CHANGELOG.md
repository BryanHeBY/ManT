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

### libmandoc-rs

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
- The new APIs are additive within the `0.9` compatibility line. A publication
  version will be assigned independently when this crate is released.

### Workspace publication transition

- The next publication will assign `0.9.1` to all seven crates once, because
  every published manifest changed its internal dependencies from exact
  `=0.9.0` requirements to explicit `^0.9.0` requirements. This packaging
  transition does not change the public Rust APIs or the
  `mant.request/v0.9` and related process protocol identifiers.
- Published `mant-protocol 0.9.0` still requires exactly `mant-ir 0.9.0`; a
  consumer that also selects `mant-ir 0.9.1` can receive two distinct crate
  versions whose Rust types are not interchangeable. Starting with the first
  independently published `mant-protocol 0.9.1`, its `mant-ir ^0.9.0`
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
