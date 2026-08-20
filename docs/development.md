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
docs/manuals/mant.md          User command, discovery, TUI, and MCP overview
docs/manuals/mant-ir.md       In-process normalized model reference
docs/manuals/mant-protocol.md Process contracts and compact MCP interface reference
docs/manuals/mant-markdown.md Supported Markdown and semantic extensions
docs/manuals/mant-roff.md     Native manual compatibility and lowering levels
docs/releasing.md            Maintainer release procedure
docs/manuals/manifest.txt     Exact self-hosted manual set shipped in releases
docs/assets/                 README screenshots and documentation assets
```

## Documentation ownership

Each topic has one authoritative home so examples and compatibility promises
do not drift between overview pages:

| Question | Authoritative document |
| --- | --- |
| What is ManT and how do I install it? | Root `README.md` and `docs/installation.md` |
| Which command, selector, or key should I use? | `mant(1)` in `docs/manuals/mant.md` |
| Which Markdown or roff constructs are retained? | `mant-markdown(7)` and `mant-roff(7)` |
| What does an in-process node mean? | `mant-ir(7)` and the `mant-ir` rustdoc |
| Which identities and projections cross host, JSON, or MCP boundaries? | `mant-protocol(5)`; generated schemas cover the structured JSON boundaries |
| How are document collections configured? | `docs/sources.md` |
| Which crate owns a behavior? | `docs/architecture/native-engine.md` and that crate's README/rustdoc |
| How is a release produced and attested? | `docs/releasing.md` |

Crate READMEs are included directly as crate-level rustdoc. Keep Rust examples
valid doctests and use repository-absolute HTTPS links for material that is
not packaged with the crate. User manuals remain ordinary Markdown so ManT can
parse and ship them as its own documentation library.

Generated paths are excluded from version control:

- `target/` and `fuzz/target/` — Cargo build output
- `dist/` — locally assembled release archives

## Testing boundaries

Rust is authoritative for parser correctness, IR and protocol contracts,
semantic option extraction, terminal presentation, process behavior, and
output rendering.
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
Its heavier concurrency check stays outside routine CI and must instrument
both languages across the FFI boundary:

```sh
rustup toolchain install nightly --profile minimal
rustup component add rust-src --toolchain nightly
crates/libmandoc-rs/scripts/check-thread-safety
```

The runner recompiles `std` with TSAN, explicitly instruments the vendored C
objects, uses an isolated target directory, and exercises both in-memory
parsing and concurrent source-relative `.so` resolution. Use `--rounds N` for
a longer local soak. It supports `x86_64` and `aarch64` Linux/glibc and macOS;
Windows retains ordinary concurrent regression coverage in CI but is outside
this TSAN runner. The sanitizer runner, patch series, and upstream checksum are
repository maintenance inputs and are intentionally absent from the published
crate, which ships only the resulting buildable vendor tree.

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

### Roff fidelity audit

The existing real-fixture catalogue also feeds an optional differential audit against the host `man(1)` and groff renderer:

```sh
cargo build --package mant
cargo build --package libmandoc-rs --example roff_ast_profile
python3 scripts/audit-roff-fidelity.py --fixtures --json /tmp/mant-fidelity.json
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man --max-pages 100
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man --max-pages-per-section 25 --findings-only
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man \
  --source-pattern '^[.]Dd' --recheck-recorded --findings-only
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man \
  --max-pages-per-section 25 --syntax-priority \
  --syntax-cache /tmp/mant-roff-syntax.json.gz \
  --syntax-report /tmp/mant-roff-syntax-report.json --findings-only
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man \
  --max-pages-per-section 12 --syntax-priority \
  --review-dir /tmp/mant-roff-review
python3 scripts/audit-roff-fidelity.py --manpath /tmp/debian-man \
  --max-pages 200 --syntax-priority --dedupe-across-corpora \
  --syntax-cache /tmp/mant-debian-syntax.json.gz \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv \
  --corpus debian-sid-amd64 --findings-only
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man --max-pages-per-section 25 \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv --corpus archlinux-host
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man --recorded-only \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv --corpus archlinux-host
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man --retry-skipped \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv --corpus archlinux-host
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man --pending-only \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv --corpus archlinux-host
```

The audit compares normalized visible tokens and contiguous token phrases, plus narrowly source-conditioned whitespace and punctuation that carry mdoc semantics. It deliberately ignores general line wrapping, indentation, blank lines, typography, headers, footers, and ManT-specific visible link targets. A `REVIEW` result is a candidate for human inspection rather than a test failure; reference renderers and ManT intentionally differ in several presentation details. Stable path sampling uses the path and `--seed`, so repeating a bounded local scan selects the same pages. Use `--max-pages-per-section` when broad coverage matters more than matching the host corpus distribution, repeat `--man-section` to focus on exact suffixes such as `1`, `3ssl`, or `8`, and repeat `--source-pattern` to require multiple multiline source shapes before sampling. Source-directed sweeps are the appropriate complement to random and AST-priority sampling when a rare macro family needs complete review.

### Roff structure audit

Content comparison intentionally normalizes away line wrapping and layout, so it cannot detect every AST-to-IR topology error. The companion audit owns a narrower internal oracle: the same libmandoc AST that ManT lowers. It compares source-aware IR against observable no-fill lines, list and definition items, table rows and spans, relative indentation, explicit breaks, and typed links; terminal geometry remains out of scope.

```sh
cargo build --package mant-engine --example roff_structure_profile
python3 scripts/audit-roff-structure.py --fixtures --json /tmp/mant-structure.json
python3 scripts/audit-roff-structure.py --manpath /usr/share/man \
  --corpus archlinux-host --replay-fidelity-records --findings-only
python3 scripts/audit-roff-structure.py --manpath /usr/share/man \
  --source-pattern '^[.]nf$' --recheck-recorded --findings-only
```

[`STRUCTURE_AUDIT.csv`](../tests/fixtures/roff/STRUCTURE_AUDIT.csv) is an independent incremental ledger, while its [guide](../tests/fixtures/roff/STRUCTURE_AUDIT.md) defines the review lifecycle. The `roff_structure_profile` example and both audit scripts are development tools: no host corpus, renderer, or batch profile is part of normal CI. CI runs only their dependency-free self-checks and the focused Rust regressions that follow a confirmed finding.

### Roff renderer-layout audit

The optional renderer-layout audit is separate from both the content and
AST-to-IR ledgers. It uses the same local `man(1)`/groff reference rendering as
the fidelity auditor, but only compares source-gated line boundaries, spacing,
and authored relative indentation; formatter-owned display gutters and body
margins are out of scope. It
does not re-run, modify, or invalidate completed `FIDELITY_AUDIT.csv` or
`STRUCTURE_AUDIT.csv` rows.

```sh
cargo build --package mant
python3 scripts/audit-roff-layout.py --manpath /tmp/new-release/share/man \
  --corpus new-release-amd64 --max-pages-per-section 20 \
  --json /tmp/mant-layout.json --findings-only
```

For a new layout pass over the exact bytes already completed by the existing
content audit, use `--replay-fidelity-records` with that corpus. This reads
`FIDELITY_AUDIT.csv` only as a source-identity index, accepting its comparable
`clean` and `review` rows but not historical `skipped` or `hard-failure`
records. The prior audit results remain immutable while the new observations
enter `LAYOUT_AUDIT.csv`.

[`LAYOUT_AUDIT.csv`](../tests/fixtures/roff/LAYOUT_AUDIT.csv) is evidence for
newly selected renderer-layout sweeps, not a claim that older
content/structure samples were retroactively checked. It is independent of
both older ledgers: a completed layout row never rewrites or upgrades their
historical conclusions. Its [guide](../tests/fixtures/roff/LAYOUT_AUDIT.md)
defines the narrow signal and review lifecycle. Do not add it to daily CI;
only its self-check and focused regressions derived from confirmed findings
belong there.

`--syntax-priority` replaces path-only ranking with deterministic greedy coverage over the actual owned libmandoc AST. The development-only `roff_ast_profile` example reports macro names, node roles and parent/child shapes, normalized list/display/font state, tables, equations, parser diagnostic classes, and rendering-relevant node flags without copying document text. It also reports bounded interaction features: a node or parent/child context paired with its flags and normalized attributes, plus attribute pairs on the same node. The sampler weights these combinations ahead of isolated features, first preferring shapes absent from the completed CSV ledger and then underrepresented and rarer forms. This distinguishes merely having seen `.SY`, a no-fill node, and a font from having exercised their exact combination.

`--syntax-report` records per-feature corpus, ledger, reused-source, and selected-page counts plus representative paths; it measures exercised parser structure, not semantic correctness. `--syntax-cache` avoids reparsing unchanged `(corpus, path, source hash)` identities and supports compact `.json.gz` files. Both the profiler response and cache carry a feature-schema identity, so changing the set of observed AST flags or shapes invalidates and rebuilds an older cache instead of silently treating stale profiles as complete. Profiling uses bounded subprocess batches and isolates an abnormal native-parser exit down to the exact page instead of losing the whole scan. Unreadable host paths remain visible as report errors but do not masquerade as syntax features.

`--review-dir` writes a local, untracked review bundle for every selected page: the decompressed source, normalized reference text, ManT text, its exact finding, and a path-safe manifest. Use a bounded syntax-prioritized selection to build a deliberate manual review batch, and never commit the bundle because it can contain third-party manuals and renderer-specific output. The regular JSON report and CSV ledger remain compact metadata rather than a copy of the reviewed corpus.

Treat fidelity loss as three related but distinct classes: an unhandled construct, a recognized construct with an unmapped operand, or an incorrectly lowered recognized construct. Structured parser/lowering diagnostics can expose the first two classes, but a zero diagnostic count never proves fidelity because the third class requires an external oracle or a focused semantic regression. The local differential audit is therefore the discovery surface; reproducible fixtures and exact Rust assertions remain the CI gate.

The optional CSV audit database makes successive local runs incremental. A page is omitted from a normal run only when its corpus name, relative path, and decompressed-source SHA-256 match a row whose automated scan completed; an upgraded page and a historical `scan_status=skipped` row are scanned again. For a newly extracted distribution or release corpus, `--dedupe-across-corpora` additionally reuses a completed record only when decompressed bytes, topic, and exact manual section all match. Sources containing `.so` or `.mso` are never reused this way because their result depends on the owning hierarchy. Reused pages remain visible in the syntax report with their originating corpus and path; they do not create misleading duplicate CSV scan rows.

Pages containing `.so` requests use the indexed-manual path with an exact derived `MANT_MANPATH`, so aliases exercise the product resolver and localized hierarchies do not fall through to the default language. Renderer failures and empty comparison surfaces are hard audit failures rather than silently accepted coverage. Automated candidates enter as `review_status=pending`; a later clean recheck clears that automated state. After inspecting the source and both renderings, mark durable conclusions `false-positive`, `confirmed-open`, or `confirmed-fixed` and explain the decision in `note`. `--retry-skipped` migrates historical gaps, `--pending-only` revisits unresolved signals, `--recheck-recorded` deliberately ignores the completion index, and `--recorded-only` re-runs every unchanged page represented by that corpus. For multiple roots, paths are relative to their common parent so same-named `man/` directories remain distinguishable. The CSV records corpus exploration; fixed fixtures and focused Rust tests, rather than the host database, remain the CI regression boundary.

The full oracle is a local and release-time discovery tool, not a per-push CI dependency. Ordinary CI runs only its dependency-free self-check plus the focused Rust regressions derived from confirmed findings. When the audit exposes real semantic loss, add the smallest licensed page to the existing source catalogue, document its provenance, and encode the confirmed behavior in the corresponding `crates/mant-engine/tests/<source>/` module or a shared assertion. Do not add an allowlist merely to silence an unexplained candidate.

Every new corpus expansion also has a manual review budget. Inspect every `REVIEW` and `HARD` result, then inspect representative clean pages that collectively cover the corpus's table forms, no-fill or display content, font changes, links or includes, and its dominant macro dialect (`man` or `mdoc`). Compare the source, the reference renderer, ManT text, and the structured result when a layout difference could hide an IR error. Record each candidate's durable conclusion in `FIDELITY_AUDIT.csv`; record the corpus-level scope and any confirmed fixes in `tests/fixtures/roff/FIDELITY_AUDIT.md`. When that review establishes that ManT preserves the source semantics more usefully than the observed terminal reference, add the exact source-hash-specific evidence to `tests/fixtures/roff/REFERENCE_RENDERER_DEVIATIONS.csv` instead of loosely calling the host formatter “wrong”. This makes the ledger evidence of both automated breadth and deliberate human inspection without turning host-specific presentation into a CI gate.

ManT intentionally does not expose this comparison as a user-facing
`mant --verify` fidelity certificate. A reference renderer is unavailable on
some supported platforms, installed macro packages and pages are host state,
and groff/mandoc presentation differences require human interpretation. A
successful comparison therefore cannot certify an arbitrary excerpt. For a
specific statement that will be quoted or published, use a source-directed
audit over the owning page, inspect the source and both renderings, and turn
any durable semantic invariant into a repository fixture.
