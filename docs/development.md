# Development guide

This guide is for contributors. User-facing installation and everyday command
examples live in the [project README](../README.md).

## Prerequisites

- Linux or macOS with local manual pages and `man`
- Bun 1.3.14
- Rust 1.88 or newer with `cargo`, `clippy`, and `rustfmt`
- GCC on Linux or Clang on macOS; set `CC` to override the selected compiler

The workspace vendors libmandoc, so installing system `mandoc` is optional.

## Start from a fresh clone

```sh
bun install
bun run dev -- git
```

`bun run dev -- <topic>` builds the release `mant` for the current host,
stages it under `bin/`, and executes that exact binary. It never depends
on a globally installed `mant`.

Run the full local verification sequence before handing off a change:

```sh
bun run build
```

It installs locked dependencies, checks the retained TypeScript reference
implementation, formats/tests/lints the Rust workspace, runs all Bun tests,
builds the unified `mant` executable, and smoke-tests it. The current-platform
artifact is written to `dist/mant`.

Focused commands are available when iterating:

```sh
bun test
bun run lint
bun run rust:test
bun run rust:lint
bun run build:mant
```

## Repository map

```text
apps/mantui/                 Retained OpenTUI reference and regression suite
  src/                       Historical process client and interactive frontend
  tests/                     Bun contract and terminal-rendering regression tests
crates/mant-ast/             Versioned document, query, outline, and schema types
crates/mant-core/            Source loading, libmandoc lowering, projections, output
crates/mant-ui/              Ratatui reader, navigation, search, and terminal styling
crates/mant/                 Mode selection, CLI, request JSON, and MCP stdio boundary
crates/libmandoc-rs/         Owned libmandoc parse API, private C shim, and vendored source
fuzz/                        Standalone cargo-fuzz workspace
tests/fixtures/              Fixed Markdown and real roff integration sources
scripts/                     Local build, compiler selection, packaging, and dev wrappers
tests/contracts/             Cross-language JSON contract fixtures (read by Rust and TS)
tests/unit/scripts/          Bun tests for the orchestration scripts
docs/architecture/           Design decisions and stable-boundary documentation
docs/manuals/                Self-hosted Markdown manual shipped in releases
docs/assets/                 README screenshots and other documentation assets
```

Generated paths are intentionally excluded from version control:

- `target/` — Cargo build output
- `bin/` — staged local native executable
- `dist/` — compiled and packaged local artifacts

## Testing boundaries

Rust owns parser correctness, AST contracts, semantic option extraction,
terminal presentation, and output rendering. Fixed real roff sources in
`tests/fixtures/roff/real/` are covered by native integration tests;
their provenance and licenses are documented in that directory. Bun tests
retain cross-language protocol and historical UI coverage while Rust tests are
authoritative for the shipped reader.

The file `docs/manuals/mant.md` is executable documentation. Native tests parse
it through the supported Markdown subset, require its embedded quick reference
and semantic options, and reject any lossy fallback diagnostic.

`libmandoc-rs` also has a self-contained package test boundary: its parser,
compression, include-policy, diagnostic, and optional `serde` tests must pass
from Cargo's staged package directory without ManT fixtures outside the crate.

When changing a versioned AST or protocol type, update its Rust contract tests
and the retained TypeScript schema consumer in the same change. Keep the stdio
boundary closed for external clients: unknown request fields and unknown
response shapes must fail before application code receives them. The native UI
does not cross that boundary; it consumes the in-memory `QueryBundle` directly.
