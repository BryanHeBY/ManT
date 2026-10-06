# CVS snapshot refresh: 2026-09-20

This is the historical record for the previous pins. The active source refresh
is documented in [REFRESH-20261006.md](REFRESH-20261006.md).

This record describes the source-only transition from `cvs-20260911` to
`cvs-20260920T122115Z`. It does not describe the later structured-rendering
refactor.

## Frozen source

- Official CVS root: `:ext:anoncvs@mandoc.bsd.lv:/cvs`
- Module: `mandoc`
- Inclusive UTC cutoff: `2026-09-20 12:21:15 UTC`
- Shipping manifest: 198 files; SHA-256
  `fd6ff088c152e57194a945d6a4deb95876fb62304008d368da35d12904afe966`
- Regression manifest: 2042 files; SHA-256
  `459ecca39e95e5aaeb8356626e2fb8aa697b9fc66b0a5a9f4b01bc0a3643c5a8`
- Complete archive: SHA-256
  `6ccee6e73346b3e2c9701ba70dc5ac0525f2dd160cfe98ffc00ad56371d28c7d`
- CVS inventory: SHA-256
  `80d92edb87e5a422f7ea695a2150fe442d8a2fefbbe6342d609b8a120fdeae81`

Two independent official checkouts produced identical manifests and archive
inputs. `SNAPSHOT.json` records the exact checkout command and CVS client
identity. `SOURCE`, `FILES`, `REGRESS_FILES`, and `CVS_INVENTORY.json` are the
tracked replay lock; ordinary Cargo builds do not contact CVS.

## Upstream delta

There are no added or removed shipping files. Exactly 24 files changed both
CVS revision and content:

| File | Revision | Classification and effect |
| --- | --- | --- |
| `Makefile` | 1.547 -> 1.549 | Build/install: edited manuals, `config.sed`, and maintenance targets. |
| `NEWS` | 1.46 -> 1.50 | Release documentation only. |
| `apropos.1` | 1.51 -> 1.52 | Install-time name substitution markers. |
| `catman.8` | 1.15 -> 1.16 | Install-time name substitution markers. |
| `cgi.c` | 1.185 -> 1.188 | CGI gzip, header, URL, and cleanup behavior; not compiled by ManT or the `mandoc` oracle. |
| `configure` | 1.87 -> 1.91 | Build: execution probes, `ETCDIR`, and `config.sed`. |
| `configure.local.example` | 1.48 -> 1.51 | Build documentation only. |
| `demandoc.1` | 1.9 -> 1.10 | Documentation wording and semantic markup. |
| `eqn.7` | 1.39 -> 1.40 | Documentation links and semantic markup. |
| `lib.in` | 1.22 -> 1.23 | Runtime data: new library names and revised `libthr` description. |
| `makewhatis.8` | 1.7 -> 1.8 | Install-time name substitution markers. |
| `man.1` | 1.42 -> 1.43 | Documentation wording. |
| `man.7` | 1.156 -> 1.157 | Install-time name substitution markers. |
| `man.cgi.3` | 1.4 -> 1.5 | CGI implementation documentation. |
| `man.cgi.8` | 1.24 -> 1.25 | CGI `head.html` documentation. |
| `man.conf.5` | 1.8 -> 1.9 | Install-time name substitution markers. |
| `mandoc.1` | 1.275 -> 1.276 | Install-time name substitution markers. |
| `mandoc.db.5` | 1.5 -> 1.6 | Install-time name substitution markers. |
| `mandoc_char.7` | 1.80 -> 1.81 | Install-time name substitution markers. |
| `mandocd.8` | 1.5 -> 1.6 | Install-time name substitution markers. |
| `mdoc.7` | 1.304 -> 1.306 | Documents additional existing FreeBSD `.Cd` uses. |
| `roff.7` | 1.126 -> 1.127 | Install-time name substitution markers; request tables unchanged. |
| `soelim.1` | 1.5 -> 1.6 | Install-time name substitution markers. |
| `tbl.7` | 1.39 -> 1.40 | Documentation semantic markup. |

`lib.in` is the only changed file that enters ManT's Cargo native archive:
`lib.c` includes it at compile time. In non-SYNOPSIS sections,
`mdoc_validate.c::post_lb` calls `mdoc_a2lib`; a match changes the owned AST,
diagnostics, and renderer output. The upstream snapshot adds `libbsdconf`,
`libgmock`, `libgtest`, `libsys`, and `libutil++`, and changes the `libthr`
description from "1:1 Threading" to "Threading". The local `libbsd` extension
remains separate. The SYNOPSIS-specific `/* -lname */` path does not consult
this table.

The new `configure` defaults to executing native probes. The authoritative
oracle retained that default (`TEST_EXECUTION=1`). Cross-build probe output
would be a different oracle identity. `Makefile.local` and `config.sed` are
generated build evidence, not CVS source files.

## Patch disposition

This section records the patch numbers at the time of this source refresh.
For the active stack and the later renumbering, see `../patches/README.md`.

All patches were replayed in `patches/series` order with `patch --fuzz=0`.
There were no offsets or rejected hunks.

| Patch | Purpose | Disposition |
| --- | --- | --- |
| 0001 | Bound memory-input UTF-8 lookahead. | unchanged |
| 0002 | Preserve unknown-encoding detection. | unchanged |
| 0003 | Preserve continued TP/TQ aliases. | unchanged |
| 0004 | Retain already-tagged mdoc heads. | unchanged |
| 0005 | Keep ohash size unsigned. | unchanged |
| 0006 | Replace input traps without leaking state. | unchanged |
| 0007 | Free native trees iteratively. | unchanged |
| 0008 | Size renderer scratch buffers safely. | unchanged |
| 0009 | Initialize optional renderer state. | unchanged |
| 0010 | Keep RFC URL bytes unsigned. | unchanged |
| 0011 | Render direct-layout sentinels. | unchanged |
| 0012 | Recognize the local `libbsd` library name. | rebased |
| 0013 | Recognize Pandoc verbatim-font spellings. | unchanged |
| 0014 | Isolate parser session state. | unchanged |
| 0015 | Add bounded memory sources and input budgets. | unchanged |
| 0016 | Bound native parser depth. | unchanged |
| 0017 | Retain executed flow boundaries. | unchanged |
| 0018 | Produce deterministic manual dates. | unchanged |
| 0019 | Isolate reference-renderer state. | unchanged |
| 0020 | Capture deterministic reference output. | unchanged |
| 0021 | Support portable memory renderers. | unchanged |
| 0022 | Apply private configuration to roff escapes. | unchanged |
| 0023 | Initialize escape-parser state. | unchanged |
| 0024 | Retain executed tbl escape state. | unchanged |
| 0025 | Retain tbl source provenance. | unchanged |
| 0026 | Track tbl-cell execution provenance. | unchanged |
| 0027 | Bound escape-parser depth. | unchanged |

Patch 0012 required only a context rebase because upstream inserted
`libbsdconf` between `libbluetooth` and `libbsdxml`. Upstream still does not
contain `libbsd`; the requirement remains. No patch was absorbed or became
obsolete. Patches 0017 and 0024-0026 remain consumed by the current lowering
path and can only be reconsidered after that path is replaced.

## Pristine oracle

The independent reference binary was built from the unpatched frozen archive
with `./configure` and `make -j4 mandoc` on Linux x86_64 using GCC 16.2.1.
Its identity is `cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1`, and its binary
SHA-256 is
`d7c58752e2586695d17f4a87cc5540b35ec65ebec51bbcb5de8c9e6786e7311d`.
The tracked attestation records `config.h`, `config.log`, `Makefile.local`,
`config.sed`, the build recipe, source lock, manifests, archive, compiler, and
binary hashes. The previous oracle remains registered as historical.

A 78-column UTF-8 non-SYNOPSIS `.Lb` probe confirmed the expected upstream
delta: `libbsdconf` renders as "Configuration File Library", `libthr` as
"Threading Library", and pristine upstream still diagnoses `libbsd` as an
unknown library and renders its generic fallback. The product patch continues
to recognize `libbsd` deliberately.

The pristine build emits existing upstream compiler warnings in files later
covered by ManT safety patches. This is expected for the unpatched oracle and
is recorded in `config.log`/build output; product builds apply the patch stack
and use the repository warning policy.

## Verification

The refresh was verified on 2026-09-20 with:

- offline archive replay and an independent live replay from the official CVS
  server at the fixed cutoff;
- registered oracle preflight against the new archive, source lock, recipe,
  configure evidence, compiler identity, and binary;
- `LIBMANDOC_RS_DENY_WARNINGS=1 cargo test -p libmandoc-rs --all-features`;
- affected `mant-codec`, `mant-engine`, CLI, real-fixture, package-list, and
  license-distribution tests;
- `scripts/check.sh --build-profile release`, including the complete workspace,
  feature matrix, independent consumers, symbol namespace, packaged-crate
  extraction tests, documentation, Clippy, fuzz-target compilation, and the
  release executable smoke test.

The first complete-gate attempt was sandboxed and stopped only because four
`mant-sources` tests could not bind a loopback test server. Re-running the same
gate with local socket permission passed, including those four tests. Dedicated
ThreadSanitizer and AddressSanitizer runners remain maintainer-only separate
jobs and were not run for this source-only refresh. No Windows or macOS runner
was available locally; their checked configurations and package boundaries
were compiled or inspected by the repository's existing cross-platform gates.
