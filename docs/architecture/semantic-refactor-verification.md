# Semantic pipeline refactor verification

Recorded on 2026-09-07. This is acceptance evidence for the bounded internal
refactor, not a new protocol contract or a claim of exhaustive correctness.

## Producers and scope

- Baseline: `df6bbc61`; preserved release executable SHA-256:
  `b511736c6c24f7dfe8680966545c2060d00593e447c4d0773dba02f851f47436`.
- Tested implementation: `1b133601e80cb2b8603adc1ed147b632aa902942`;
  release executable SHA-256:
  `419dc29bdf0f812340523e67fa355187102a9a1fe5cecbb9a2facbcbf030d819`.
- Host: Linux 6.18.33.2-microsoft-standard-WSL2; Rust/Cargo 1.98.0.
- No dependency, release version, wire schema or audit CSV changed.

The five functional commits preserve the same content tree and query policies:

1. `3dc28cfd`: original list/item identities and one source-local annotation
   binding table; declarations no longer use content-start positions or line
   numbers as interchangeable identities.
2. `cba765ca`: separate annotation traversal, head signatures, Markdown name
   grammar, rejection diagnostics and directive syntax. Native inference and
   explicit Markdown admission remain distinct.
3. `0961d956`: reuse an immutable document-bound validation sidecar for
   explanation relationships and final diagnostics; no global cache or skipped
   external-producer checks.
4. `b9ce5381`: separate CLI format dispatch, terminal adapters, scope
   presentation and tests; shared semantic renderers remain authoritative.
5. `1b133601`: read-only Markdown artifact text and node mappings, with
   consuming extraction and a lazy anchor cache over final bytes.

Regression additions cover original markers across LF/CRLF/CR, duplicate and
invalid relationship findings in one validation snapshot, rejection of source
mutation while a snapshot is live, and final trimmed node/anchor coordinates.
Existing nested declaration, export, CLI/MCP and PTY oracles were retained.

## Gates

- `bash scripts/check.sh`: succeeded, including default workspace tests,
  optional native features, profiler tests, packaged source sets, native symbol
  audit, rustdoc with denied warnings, strict Clippy, fuzz compilation and
  release build/smoke.
- `cargo test --locked --workspace --all-features`: 1,293 passed, zero failed,
  six existing ignored tests, 55 reported suites including doctests.
- Fixture projection audit: 37 clean pages and 106 excerpts.
- Fixture target audit: 37 clean pages.
- Fixture semantic audit: 37 clean pages, 10,725 entries; zero ordinal,
  retained-ordinal-definition, empty-entry, value-domain or conversion violations.

The initial sandboxed gate stopped at four loopback HTTP fixture binds with
`EPERM`. The complete gate and all-feature tests were rerun with local listening
permitted and passed. No product change was made to accommodate that restriction.

## Exact output comparison

Both binaries consumed the same current input paths from the repository root:
all 37 real roff fixtures and all five `docs/manuals/*.md` manuals. Each input
was supplied with `--input PATH` and these argument sets:

- `--format json`
- `--format text`
- `--format text --color always`
- `--format markdown --preserve-anchors`
- `--outline --outline-entries all --format json`
- `--search help --limit 5 --format json`
- `--explain=--help --limit 3 --explain-content-bytes 4096 --format json`
- `--format man` for the 37 roff inputs only.

All 331 comparisons matched exactly in exit status, stdout and stderr. Nothing
filtered identities, diagnostics, source paths, ANSI sequences or whitespace.
The JSONL record names each input and full argv and records input, stdout and
stderr SHA-256 values for both binaries, rather than anonymous output hashes.

## Small performance sample

After the other tests finished, each binary was warmed once and run seven times
per query, alternating old/new order. Each query used `--input PATH`,
`--explain=QUERY` and `--format json` with default explanation budgets.

| Fixture / query | Baseline median | Refactored median |
| --- | --- | --- |
| `archlinux/gcc.1.gz` / `--help` | 194.2 ms | 186.2 ms |
| `archlinux/gcc.1.gz` / `MANT_ABSENT_71923` | 190.4 ms | 176.3 ms |
| `fedora44/sh.1.zst` / `history` | 50.0 ms | 46.6 ms |

Paths are beneath `tests/fixtures/roff/real/`. These are end-to-end local
samples, not isolated collector benchmarks, RSS measurements or universal
speedup guarantees.

## Local evidence and limitations

The following full logs are temporary host artifacts; their hashes identify
this run but do not imply that the artifacts are distributed with the repository.
The commands, producers, scope and results above are retained here for review.

| Local artifact | SHA-256 |
| --- | --- |
| `/tmp/mant-refactor-final-check-unrestricted.log` | `98d69621e1bd93a7b61acae46d75dec8aa8cb734916d64c4e17c192f0ecce904` |
| `/tmp/mant-refactor-final-all-features.log` | `35aeefa6f56eae450e7b0a4e4db630c084c47c9d889207481b900d7a34903320` |
| `/tmp/mant-refactor-acceptance.BvfGRM/output-comparison.jsonl` | `b423034f7603d6636b217a992123d4ddebde817675376b38b130162683b274c5` |
| `/tmp/mant-refactor-diff.py` | `9397be65298bc5170d37660bd90f1c1962e56389b0c2409903f8fc30421828ed` |

No native Windows/MSVC or macOS validation, new sanitizer run, 45,036-page host
corpus rescan, or expanded terminal/installation fault injection was performed.
The older refactoring review's remaining platform and failure-path evidence
items are not closed by this narrower verification. No push or release occurred.
