# Development guide

This guide is for contributors. User-facing installation and everyday command
examples live in the [project README](../README.md).

## Prerequisites

- Linux or macOS with local manual pages and `man`
- Bun 1.3.14
- Rust 1.85 or newer with `cargo`, `clippy`, and `rustfmt`
- GCC on Linux or Clang on macOS; set `CC` to override the selected compiler

The workspace vendors libmandoc, so installing system `mandoc` is optional.

## Start from a fresh clone

```sh
bun install
bun run dev -- git
```

`bun run dev -- <topic>` builds the release `mant-cli` for the current host,
stages it under `native/bin`, sets `MANT_CLI_PATH`, and starts the Bun TUI. It
never depends on a globally installed `mant-cli`.

Run the full local verification sequence before handing off a change:

```sh
bun run build
```

It installs locked dependencies, checks TypeScript, formats/tests/lints the
Rust workspace, builds the native CLI, runs all Bun tests, compiles `mant`, and
smoke-tests both binaries. The current-platform artifacts are written to
`dist/mant` and `dist/mant-cli`.

Focused commands are available when iterating:

```sh
bun test
bun run lint
bun run rust:test
bun run rust:lint
bun run build:mant-cli
```

## Repository map

```text
src/                         Bun entry point, native-client boundary, and OpenTUI UI
  cli/                       Interactive `mant` grammar and error boundary
  native/                    `mant-cli` process discovery and response validation
  ui/                        Sidebar, content, search, menus, and terminal layout
native/
  crates/mant-ast/           Versioned document, query, outline, and schema types
  crates/mant-core/          Source loading, libmandoc lowering, projections, output
  crates/mant-cli/           Agent/script CLI and stdin JSON boundary
  crates/mant-mandoc-sys/    Safe Rust boundary around vendored libmandoc C code
  vendor/                    Pinned upstream mandoc source and license
scripts/                     Local build, compiler selection, packaging, and dev wrappers
tests/                       Bun unit/integration/TUI tests and fixed roff fixtures
docs/architecture/           Design decisions and stable-boundary documentation
docs/assets/                 README screenshots and other documentation assets
```

Generated paths are intentionally excluded from version control:

- `native/target/` — Cargo build output
- `native/bin/` — staged local native executable
- `dist/` — compiled and packaged local artifacts

## Testing boundaries

Rust owns parser correctness, AST contracts, semantic option extraction, and
rendering. Fixed real roff sources in `tests/fixtures/roff/real/` are covered
by native integration tests; their provenance and licenses are documented in
that directory. Bun tests cover the process protocol, schema validation, and
the terminal UI's rendering, search, and navigation behavior.

When changing a versioned AST or protocol type, update its Rust contract tests
and the TypeScript schema consumer in the same change. Keep the stdio boundary
closed: unknown request fields and unknown response shapes must fail before UI
code receives them.
