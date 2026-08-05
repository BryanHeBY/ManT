# Development guide

This guide is for contributors. User-facing installation and command examples
live in the [project README](../README.md).

## Prerequisites

- Linux, macOS, or Windows
- Rust 1.88 or newer with `cargo`, `clippy`, and `rustfmt`
- For Unix native-manual work: GCC on Linux or Clang on macOS, zlib
  development headers, and local manual source trees

The workspace vendors libmandoc and maintains its own manual index, so system
`man` and `mandoc` executables are not prerequisites. Markdown parsing and the
deterministic fixture suite do not require installed manual sources either.

## Start from a fresh clone

Build and run the unified command directly through Cargo:

```sh
cargo run --locked -p mant -- git
cargo run --locked -p mant -- README.md
```

Run the complete local verification boundary before handing off a change:

```sh
bash scripts/check.sh
```

On Windows, run the portable product boundary from PowerShell:

```powershell
.\scripts\check-windows.ps1
```

It tests `mant-ast`, `mant-core`, `mant-ui`, and `mant` without building the
Unix-only `libmandoc-rs` workspace member or roff fixture suites.

Those four portable crates are also the workspace `default-members`, so a bare
`cargo build`, `cargo test`, or `cargo clippy` works on Windows. Unix verification
deliberately passes `--workspace` to include the standalone `libmandoc-rs`
package and its native parser tests.

The script checks formatting, runs every workspace test, runs clippy with all
targets and features, builds the optimized executable, and smoke-tests its
human and JSON surfaces. The result is `target/release/mant`.

Focused commands are useful while iterating:

```sh
cargo fmt --all --check
cargo test --locked --workspace
cargo test --locked -p mant-ui
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo build --locked --release -p mant
```

## Repository map

```text
Cargo.toml                    Root Rust workspace and shared dependency policy
crates/mant-ast/             Versioned document, query, outline, and schema types
crates/mant-core/            Source loading, libmandoc lowering, projections, output
crates/mant-ui/              Ratatui reader, navigation, search, and terminal styling
crates/mant/                 Mode selection, CLI, request JSON, and MCP stdio boundary
crates/libmandoc-rs/         Owned libmandoc parse API, private C shim, vendored source
fuzz/                        Standalone cargo-fuzz workspace
tests/contracts/             Stable JSON contract fixtures consumed by Rust tests
tests/fixtures/              Fixed Markdown and real roff integration sources
scripts/check.sh             Canonical local and CI verification sequence
scripts/package-release.sh   Reproducible Linux release archive assembly
scripts/package-release.ps1 Windows x64 ZIP assembly
docs/architecture/           Design decisions and stable-boundary documentation
docs/manuals/                Self-hosted Markdown manual shipped in releases
docs/assets/                 README screenshots and documentation assets
```

Generated paths are excluded from version control:

- `target/` and `fuzz/target/` — Cargo build output
- `dist/` — locally assembled release archives

## Testing boundaries

Rust is authoritative for parser correctness, AST contracts, semantic option
extraction, terminal presentation, process behavior, and output rendering.
Fixed real roff sources in `tests/fixtures/roff/real/` are covered by native
integration tests; their provenance and licenses are documented in that
directory.

The file `docs/manuals/mant.md` is executable documentation. Tests parse it
through the supported Markdown subset, require its embedded quick reference
and semantic options, and reject lossy fallback diagnostics.

`libmandoc-rs` also has a self-contained package boundary: its parser,
compression, include-policy, diagnostics, and optional `serde` tests must pass
from Cargo's staged package directory without fixtures from sibling crates.

When changing a versioned AST or protocol type, update the Rust contract,
generated-schema, process, and projection tests in the same change. External
stdio remains a closed boundary: unknown request fields and incompatible
response shapes fail before application code receives them. The in-process UI
consumes the typed `QueryBundle` directly and never serializes it first.

## Native fixtures

Do not replace a real distribution fixture with a hand-written approximation
when fixing parser or lowering behavior. Add the smallest redistributable real
source that reproduces the problem, record its origin and license under
`tests/fixtures/roff/real/`, and assert the normalized structure rather than a
terminal screenshot. Renderer tests then verify the same AST independently.
