# Development guide

This guide is for contributors. User-facing installation and command examples
live in the [project README](../README.md).

## Prerequisites

- Linux with glibc, macOS, or Windows
- Rust 1.88 or newer with `cargo`, `clippy`, and `rustfmt`
- For native-manual work: GCC on Linux, Clang on macOS, or MSVC on Windows;
  Unix also needs zlib development headers

The workspace vendors libmandoc and maintains its own manual index, so system
`man` and `mandoc` executables are not prerequisites. Markdown parsing and the
deterministic fixture suite do not require installed manual sources either.

## Start from a fresh clone

Build and run the unified command directly through Cargo:

```sh
cargo run --locked -p mant -- git
cargo run --locked -p mant -- --input README.md
```

Run the complete local verification boundary before handing off a change:

```sh
bash scripts/check.sh
```

On Windows, run the native product boundary from PowerShell:

```powershell
.\scripts\check-windows.ps1
```

It tests `libmandoc-rs`, `mant-ir`, `mant-protocol`, `mant-sources`, `mant-engine`, `mant-ui`, and `mant`,
including the shared roff fixture suites.

The product crates are workspace `default-members`, so a bare `cargo build`,
`cargo test`, or `cargo clippy` works on Windows. Both platform verification
scripts include the standalone `libmandoc-rs` package and native parser tests.

The script checks formatting and installer syntax, runs every workspace test,
runs clippy with all targets and features, builds the optimized executable,
and smoke-tests its human and JSON surfaces. The result is
`target/release/mant`.

CI uses `--build-profile debug` for the final smoke test because its test and
Clippy steps have already populated that profile. Local checks keep `release`
as the default, and tagged publication performs a separate optimized build.
When an exact commit has already completed every full CI job on `dev`, a
fast-forward push of that commit to `main` verifies and reuses the recorded
check suite instead of executing it twice. Direct pushes to `main`, pull
requests, and manual runs still execute the full suite.
macOS adds native compile and test coverage without repeating the Linux lint
pass. Windows retains its full native verification boundary for Windows-only
path, shell, packaging, and parser behavior.

Each native job caches downloaded crates and compiled third-party dependencies
using a key derived from its Rust compiler, Cargo manifests, lockfiles, and
compiler environment. Workspace crates and incremental artifacts are excluded
to keep restore time and repository cache use bounded. Pull requests may
restore an existing cache but only branch pushes persist new entries.

Focused commands are useful while iterating:

```sh
cargo fmt --all --check
cargo test --locked --workspace
cargo test --locked -p mant-ui
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo build --locked --release -p mant
```

Dependency policy is declared in `deny.toml`. CI runs cargo-deny across all
features and every supported target family to reject known vulnerabilities,
yanked packages, unapproved licenses, wildcard requirements, and dependencies
from untrusted registries or Git repositories. With cargo-deny 0.20.2
installed, run the same audit locally:

```sh
cargo deny check
```

Native distribution notices are generated from the same locked, multi-target
graph with cargo-about 0.9.1. After changing dependencies, regenerate the
checked-in report and review its package and license mapping:

```sh
scripts/generate-rust-licenses.sh
```

Pull requests also use GitHub dependency review. Dependabot supplies weekly
version updates, while security updates, secret scanning with push protection,
and private vulnerability reporting are repository settings rather than files
in the checkout. OpenSSF Scorecard publishes a weekly, externally visible
assessment and uploads its findings to GitHub code scanning.

## Repository map

```text
Cargo.toml                    Root Rust workspace and shared dependency policy
deny.toml                     Dependency license, advisory, source, and ban policy
about.toml                    Distributable Rust dependency notice policy
LICENSE                       Apache-2.0 terms for ManT-authored work
THIRD_PARTY_NOTICES.md        Repository-wide third-party distribution map
THIRD_PARTY_LICENSES.html     Generated Rust dependency license report
SECURITY.md                   Supported versions and private reporting policy
crates/mant-ir/               Semantic IR, ResolvedContent, paths, visitors, validation, indexes
crates/mant-protocol/         Versioned request/response DTOs and JSON Schema
crates/mant-sources/          Local Markdown registry and transactional source updates
crates/mant-engine/           Resolution, lowering, projections, search, and rendering
crates/mant-ui/               Ratatui reader, navigation, search, and terminal styling
crates/mant/                  Mode selection, CLI, request JSON, and MCP stdio boundary
crates/libmandoc-rs/          Owned libmandoc parse API, private C shim, vendored source
fuzz/                        Standalone cargo-fuzz workspace
tests/contracts/             Stable JSON contract fixtures consumed by Rust tests
tests/fixtures/              Fixed Markdown and real roff integration sources
scripts/check.sh             Canonical local and CI verification sequence
scripts/build-and-smoke.sh   Unix debug/release product build and smoke test
scripts/build-and-smoke.ps1 Windows debug/release product build and smoke test
scripts/find-successful-ci.sh  Exact-commit full CI verification for automation
scripts/generate-rust-licenses.sh  Rebuild the locked Rust license report
scripts/install.sh           Latest-release installer for Linux and macOS
scripts/install.ps1          Latest-release installer for Windows x64
scripts/update-reader-screenshot.sh  Host-stable Linux README screenshot capture
scripts/package-release.sh   Reproducible Linux release archive assembly
scripts/package-release.ps1 Windows x64 ZIP assembly
docs/architecture/           Design decisions and stable-boundary documentation
docs/installation.md         User installation methods and platform requirements
docs/sources.md              Markdown source configuration and update behavior
docs/protocol.md             Versioned process and MCP interface reference
docs/releasing.md            Maintainer release procedure
docs/manuals/                Self-hosted Markdown manual shipped in releases
docs/assets/                 README screenshots and documentation assets
```

Generated paths are excluded from version control:

- `target/` and `fuzz/target/` — Cargo build output
- `dist/` — locally assembled release archives

## Testing boundaries

Rust is authoritative for parser correctness, IR and protocol contracts, semantic option
extraction, terminal presentation, process behavior, and output rendering.
Fixed real roff sources in `tests/fixtures/roff/real/` are covered by native
integration tests; their provenance and licenses are documented in that
directory.

The file `docs/manuals/mant.md` is executable documentation. Tests parse it
through the supported Markdown subset, require its embedded quick reference
and semantic entries, and reject lossy fallback diagnostics.

### Fuzzing

The standalone `fuzz/` workspace keeps fuzz-only dependencies out of the
shipped crates. CI compiles every target on stable Rust; randomized execution
is a local or scheduled activity because a short nondeterministic CI run is not
a reliable security boundary.

The maintained targets follow externally supplied data rather than crate
boundaries:

- `markdown_parse` is the high-throughput Markdown parser target.
- `markdown_pipeline` exercises Markdown parsing, semantic selectors, search,
  outlines, excerpts, and every output renderer.
- `tldr_page` covers the TLDR subset and command-token parser independently.
- `roff_pipeline` crosses the native libmandoc boundary and then exercises the
  same projections and renderers as Markdown.
- `catalog_query` covers bounded literal/regex discovery, hierarchical paths,
  relevance ordering, filters, and pagination without touching the host file
  system.

Curated seeds under `fuzz/corpus/` reach semantic comments, links, TLDR
placeholders, man and mdoc macros, and hierarchical catalog names quickly.
All targets cap individual inputs at 64 KiB so time is spent exploring syntax
rather than repeatedly rendering oversized documents. File discovery, archive
transactions, network downloads, and terminal event handling remain in
deterministic unit and integration tests because byte mutation cannot model
their state transitions faithfully.

Install `cargo-fuzz` and a nightly toolchain, then run every target serially.
The first argument is the number of seconds per target; additional arguments
select individual targets:

```sh
cargo install cargo-fuzz --locked
rustup toolchain install nightly --profile minimal
scripts/fuzz.sh 60
scripts/fuzz.sh 300 roff_pipeline markdown_pipeline
```

Minimize any artifact with `cargo fuzz tmin`, turn the minimized input into a
named regression test, and only then discard or archive the generated corpus.

On Linux, regenerate the README reader image with:

```sh
scripts/update-reader-screenshot.sh
```

The script builds the release executable, registers the repository's ManT
manual in an isolated XDG hierarchy, opens it in a fixed Xvfb/xterm surface,
activates View → Expand All, and captures the result. It requires Xvfb, xterm,
xdotool, Fontconfig, and ImageMagick; the pinned JetBrains Mono files and their
OFL-1.1 license live under `docs/assets/fonts/`. The script pins its font,
geometry, terminal settings, and interaction sequence. The rendering tools
come from the host, however, so byte-identical captures are expected only with
the same host toolchain. Always inspect the resulting image before committing
it.

`libmandoc-rs` also has a self-contained package boundary: its parser,
compression, include-policy, diagnostics, and optional `serde` tests must pass
from Cargo's staged package directory without fixtures from sibling crates.

When changing a versioned IR projection or protocol type, update the Rust contract,
generated-schema, process, and projection tests in the same change. External
stdio remains a closed boundary: unknown request fields and incompatible
response shapes fail before application code receives them. The in-process UI
consumes `mant_ir::ResolvedContent` directly and never serializes it first.

## Native fixtures

Do not replace a real distribution fixture with a hand-written approximation
when fixing parser or lowering behavior. Add the smallest redistributable real
source that reproduces the problem, record its origin and license under
`tests/fixtures/roff/real/`, and assert the normalized structure rather than a
terminal screenshot. Renderer tests then verify the same IR independently.
