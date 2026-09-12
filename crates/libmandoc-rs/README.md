# libmandoc-rs

`libmandoc-rs` is a safe Rust ownership boundary around a pinned copy of
[libmandoc](https://mandoc.bsd.lv/).  It parses `man(7)`, `mdoc(7)`, roff,
`tbl(7)`, and `eqn(7)` input into an owned syntax tree, so callers never need
to depend on libmandoc's private C structures or parser lifetime.

## What this crate provides

- A fully owned AST with source locations, macro roles, display/list metadata,
  resolved stateful enclosures, table cells, equations, and validated
  same-document tags.
- A `Parser` API whose caller-controlled `.so` policy defaults to denial.
- Explicit `man`/`mdoc` input selection without changing the compatible
  `ParseOptions` shape.
- Bounded, read-only `SourceBundle` trees for portable in-memory `.so`
  expansion without filesystem fallback.
- Structured non-fatal diagnostics and typed source/decompression failures.
- Top-level uncompressed, gzip, and zstd manual sources.
- Concurrent parser calls with thread-local upstream and shim state.
- An optional `render` feature exposing bounded upstream ASCII, deterministic
  UTF-8, and HTML reference output without writing to process standard output.

The default crate remains a parser layer only. It intentionally does not
locate system manual pages, interpret application-specific section models, or
run a pager. The optional reference renderers format the native tree in the
same call that parses it; they do not turn the owned Rust AST into a second
document model, and `ManT`'s existing engine integration remains unchanged.

## Boundary model

```text
plain / gzip / zstd source
          │
          v
Rust transport and policy ──> private C shim ──> libmandoc cvs-20260911
          ^                         │                 │
          ├─ owned ParseReport <────┘                 │
          │  ├─ Document syntax tree                  │
          │  └─ structured diagnostics                │
          └─ bounded RenderReport <───────────────────┘  (`render` feature)
             ├─ complete reference output
             └─ structured diagnostics
```

The returned tree describes validated roff syntax: macro names, node roles,
fonts, lists, displays, stateful enclosure delimiters, tables, equations,
locations, and tags. It is not
`ManT`'s source-neutral document IR. Consumers that want normalized sections,
semantic entries, or typed links should use `mant-codec` and `mant-ir` instead.
`mant-render` formats that IR; `mant-engine` composes local loading and queries.

All bundled parser and compatibility definitions are compiled under the
`mant_vendored_*` namespace; only the private `mant_mandoc_*` Rust/shim bridge
remains separately named. Linking this crate therefore does not inject generic
symbols such as `strlcpy`, `ohash_init`, or `mparse_alloc` into a consumer's
native symbol namespace. Cargo's `links = "libmandoc_rs"` key still permits
only one `libmandoc-rs` version in a dependency graph. A breaking pre-1.0
upgrade must consequently be coordinated across every dependent crate rather
than relying on parallel `0.x` versions.

Repository CI verifies the final native archive with GNU `nm` on Linux.
macOS and Windows still compile the same prefix map and exercise linking,
parsing, and rendering, but do not claim an equivalent exported-symbol scan;
their native builds instead treat warnings as errors for the supported
toolchains.

The shim retains the completed native parser only during the synchronous FFI
transfer. Each native node and table cell is exposed as a shallow borrowed
snapshot and copied directly into the public Rust tree; no borrowed pointer
escapes the call and no intermediate heap-owned C AST is materialized. The
private parser handle is destroyed on the calling thread before `Parser`
returns, while the returned report remains fully owned and freely movable.

Within that private boundary, `ffi::session` owns the native document drop
guard and keeps bundle paths and source bytes alive for the call;
`ffi::owned` transfers the syntax tree, while `ffi::render` copies bounded
reference output using the same guard. A failed native call releases its own
session without invalidating previously returned reports. Raw declarations
and the Windows root callback remain private to the FFI boundary; none of
these internal modules is a consumer-facing API.

Table cells expose their effective `TableCellKind`: layout rules override data,
and connecting/isolated single/double rules remain distinguishable. A rule may
retain a native text payload for inspection; consumers must not print it or
recover discarded source content as though it were a text cell.

## Basic use

```rust,no_run
use libmandoc_rs::Parser;

let report = Parser::default().parse_bytes(
    "hello.1",
    b".TH HELLO 1\n.SH NAME\nhello \\- example manual\n",
)?;

println!("{:?}", report.document.macro_set);
for diagnostic in report.diagnostics {
    eprintln!("{:?}: {}", diagnostic.level, diagnostic.message);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `Parser::parse_bytes` if the caller owns the source transport. Its auto
mode recognizes plain input and zstd frames; callers must decompress gzip byte
streams first or pass the file to `parse_file`. Unix can retain libmandoc's
native gzip file transport; Windows decodes gzip files in Rust before entering
the memory parser and preserves libmandoc's `<path>.gz` fallback when the
requested top-level path does not exist.

`IncludePolicy::Deny` is the default. On Unix, `SourceTree` preserves
libmandoc-compatible lookup beside the source, at the surrounding manual-tree
root, and finally through the process working directory. It is intended for
trusted installed manual trees, not as a containment boundary, and remains
Unix-only. `Root(path)` is the strict cross-platform policy: it resolves `.so`
requests only below a caller-approved directory, rejects absolute and lexical
parent paths, refuses to traverse symbolic links or Windows reparse points
below that root, and never falls back to the process working directory. The
approved root itself may be a link. Unix opens included files relative to
directory descriptors; Windows reads them through the Rust boundary, verifies
the opened file's final path remains below the approved root, and then passes
owned bytes to memory-only libmandoc. Both an explicit `.so target.gz` and the
usual `.so target` fallback to `target.gz` are decoded on Windows. To avoid
host source-file access, build a `SourceBundle` of normalized relative paths
and call `parse_bundle`; exact bundle paths and paths beside the including
source are resolved without callbacks or filesystem fallback. Harmless `.`
components in an `.so` request are normalized before either the virtual bundle
or strict Windows root is consulted; `..`, absolute paths, backslashes, and
empty components remain rejected. Diagnostic
capture currently uses one
private anonymous temporary file per native call on platforms where
`tmpfile(3)` is filesystem-backed. If that capture cannot be created, the call
returns a typed parse/render failure instead of writing diagnostics to the host
process's standard error.

```rust
use libmandoc_rs::{Parser, SourceBundle};

let mut sources = SourceBundle::new();
sources.insert("man1/hello.1", b".so shared/hello.inc\n".to_vec())?;
sources.insert(
    "man1/shared/hello.inc",
    b".TH HELLO 1\n.SH NAME\nhello \\- virtual manual\n".to_vec(),
)?;

let report = Parser::default().parse_bundle("man1/hello.1", &sources)?;
assert_eq!(report.document.metadata.title.as_deref(), Some("HELLO"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Parser::with_input_format` can force `InputFormat::Man` or
`InputFormat::Mdoc` when the caller already knows the source language. The
default remains compatible automatic detection, and the input selection is
kept outside `ParseOptions` so existing struct literals continue to compile.
`Parser::with_mdoc_operating_system` similarly pins the fallback value for an
argument-less mdoc `.Os`; explicit `.Os name` source text still wins. Without
that override, Unix retains upstream `uname(3)` behavior and Windows retains
its configured `Windows` value, so consumers requiring byte-reproducible bare
`.Os` metadata or rendering should set the override explicitly. Libmandoc
continues to infer its OpenBSD/NetBSD validation dialect from the selected
name, matching the upstream `-I os=...` boundary.

The vendored parser subset and its include shim make all mutable parse state
thread-local, so independent `Parser` calls may run concurrently. A `Parser`
value is inexpensive immutable configuration; this guarantees parallel calls,
not recursive re-entry through a caller callback on the same OS thread. The
target configurations also lock roff syntax character classes to ASCII, and
validated manual dates use fixed English month names, so a host process calling
`setlocale` cannot change the owned AST, diagnostics, or renderer bytes.
Windows supplies the same permissive date parsing, normalization, and
pre-epoch UTC conversion used by the supported Unix targets. macOS uses the
crate's thread-local program-name compatibility layer rather than changing the
host process's global program name. The Rust ownership transfer and native
equation expansion stop descending after 256 syntax-tree or equation-box
levels. Pathological input beyond either
defensive cap still returns a successful, finite report and omits deeper
descendants, while appending an explicit warning to `ParseReport::diagnostics`;
ordinary manuals remain far below both limits. These wrapper-generated
warnings return `DiagnosticCode::SyntaxTreeDepthLimit` or
`DiagnosticCode::EquationTreeDepthLimit` from `Diagnostic::code()`; native
libmandoc findings retain their severity and message but do not invent a
machine code. The additive method keeps the existing public diagnostic fields
and optional Serde shape unchanged for compatible patch upgrades.

A separate native construction guard stops input dispatch after a syntax
node exceeds 512 parent levels, before end-of-document validation. Such input
returns a parse/render error, not a partial report. Native reference rendering
rejects syntax or equation trees beyond 256 levels before entering a formatter;
its output byte budget is independent of this stack budget. Syntax and equation
cleanup use iterative traversal, including rejected and truncated inputs.

Enable the optional `serde` feature to derive `Serialize` and `Deserialize`
for the public AST, parser configuration, reports, diagnostics, and errors.

Enable the default-off `render` feature to use `Renderer`. `RenderFormat::Ascii`
produces portable 7-bit terminal text with traditional backspace overstrikes,
`RenderFormat::Utf8` uses locked Rust Unicode cell widths without reading or
changing the process locale, and
`RenderFormat::Html` produces either a complete document or a fragment. Every
call has a configurable byte cap (8 MiB by default, 64 MiB maximum), and an
overflow returns an error rather than a partial result. Output is captured in
a per-thread native sink, so concurrent calls neither share renderer state nor
write to the process's `stdout`. `render_file`, `render_bytes`, and
`render_bundle` retain the corresponding parser transport and `.so` policies.

Reference output follows the pinned CVS formatter: its default terminal body
indent is five columns, and HTML uses semantic section containers and
accessible document structure. These native reference bytes are distinct from
`ManT`'s source-neutral text and TUI layout. The owned public AST shape and the
source, include, output-budget, and session-isolation contracts are unchanged
by the baseline selection. `LIBMANDOC_VERSION` reports `cvs-20260911`.

```rust,no_run
# #[cfg(feature = "render")]
# {
use libmandoc_rs::{RenderFormat, Renderer};

let report = Renderer::new(RenderFormat::Html)
    .with_html_fragment(true)
    .with_max_output_bytes(256 * 1024)
    .render_bytes("hello.1", b".TH HELLO 1\n.SH NAME\nhello \\- example\n")?;
assert!(report.output.contains("hello"));
# }
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Compression contract

For `parse_file`, `Compression::Auto` selects Rust zstd decoding for a `.zst`
suffix. On Windows it also selects Rust gzip decoding for a `.gz` suffix; on
Unix all other paths go through libmandoc's native file reader, including its
gzip detection. If a Windows auto-mode path is absent, the same path with an
appended `.gz` suffix is tried before returning a read error. Use
`Compression::Zstd` to force zstd decoding when a file has
another suffix. For `parse_bytes`, auto mode recognizes zstd magic and plain
input, not gzip; callers must decompress gzip byte streams themselves.
`Compression::Plain` bypasses top-level compression detection. Other
compression formats are not part of this crate's supported contract. Under
`IncludePolicy::Root`, an unresolved `.so name` also tries `name.gz`; Windows
decompresses that included source in Rust before parsing it from memory. Every
Rust-managed zstd or gzip decode is capped at 16 MiB of complete uncompressed
source and returns a typed decompression failure instead of partial bytes on
overflow. Unix native file/gzip transport retains libmandoc's own limits;
`ManT` applies its separate 16 MiB source budget before that product boundary.

## Vendor layering

The vendored C source at `vendor/mandoc-cvs-20260911/` is derived from the
official mandoc CVS module at 2026-09-11 08:00:00 UTC with ordered local
patches applied. `upstream/SOURCE` pins the checkout; `upstream/FILES`
records the SHA-256 and CVS revision of each of the 198 upstream files.
End-user `cargo build` compiles this tree directly; no network access,
CVS client, or external patch tool is required.

The pinned baseline intentionally no longer recognizes `.St -xsh4.2`.
Migrate that nonportable alias to `.St -xpg4.2`. The former produces an
`unknown standard specifier` error diagnostic and no generated standard text;
the replacement expands to the X/Open Portability Guide, Issue 4, Version 2.
This follows upstream `st.c` revision 1.17 (2022-01-13): the portable spelling
already existed, and groff never supported `-xsh4.2`. No compatibility alias is
reintroduced by this crate.

The local thread-safety patch moves remaining mutable parser-global slots in
the compiled libmandoc subset into static thread-local storage. It uses C11 TLS
on Linux and macOS, and `__declspec(thread)` on Windows/MSVC; macOS uses the
thread-local program-name compatibility layer without changing host state.
Date-only metadata is converted without process-global timezone state, while
the special current-date form uses the platform's reentrant local-time API.

From a `ManT` repository checkout, maintainers use `scripts/sync-vendor` to
regenerate the vendor tree while working in `crates/libmandoc-rs/`:

```sh
./scripts/sync-vendor           # download, patch, replace vendor/
./scripts/sync-vendor --verify  # CI: check vendor/ matches upstream + patches
./scripts/sync-vendor --verify --archive /path/to/upstream.tar.gz  # offline
```

The vendor synchronizer reads one `upstream/SOURCE` and one ordered
`patches/series`. `kind = release` locks an HTTPS archive URL, SHA-256 and
version. `kind = cvs` instead locks the official CVS root, module, UTC checkout
date, archive root, version label and a checksummed file manifest. Manifest rows
are tab-separated `sha256`, CVS `revision`, and relative `path`; they cover the
complete vendored source subset, excluding `regress/` and CVS administration.
The version label selects `vendor/mandoc-<version>/`; changing upstream baselines
updates the same source declaration and patch series, with previous states kept
in Git rather than parallel baseline directories.

`--verify` reconstructs that fixed tree and compares it with checked-in vendor
contents. A CVS checkout uses the pinned server key in `upstream/known_hosts`,
never a moving HEAD; `CVS=/path/to/cvs` selects a non-default client executable.
`--archive` avoids network access and verifies the same release archive checksum
or CVS file manifest before replay. The maintainer tool requires Python 3 and
`patch`, plus `curl` for online release retrieval or CVS and SSH for online CVS
retrieval. None of these tools is needed by an end-user Cargo build.
Semantic parser changes need a Rust test with the smallest useful roff input;
portability patches are covered by the relevant target CI jobs.

The sanitizer stress suite is also repository-only and intentionally stays out
of routine CI. It rebuilds the Rust standard library, this crate, and the
vendored C objects with `ThreadSanitizer` instrumentation, then drives
concurrent memory, source-tree, virtual-bundle, and renderer sessions:

```sh
rustup toolchain install nightly --profile minimal
rustup component add rust-src --toolchain nightly
./scripts/check-thread-safety
./scripts/check-thread-safety --rounds 256
./scripts/check-address-safety
```

The runner supports `x86_64` and `aarch64` Linux/glibc and macOS hosts, uses
an isolated Cargo target directory, stops on the first race, and verifies that
the C archive contains TSAN callbacks. Its tests are ignored by ordinary
`cargo test` because an uninstrumented stress pass cannot establish race
freedom. Windows runs ordinary cross-thread regression tests in CI, but this
TSAN runner does not support Windows.

`check-address-safety` uses the same mixed Rust/C sanitizer setup for exact
memory-only input boundaries, including truncated UTF-8, modeline and encoding
declarations at the final source byte, owned-tree traversal after native
parser release across the licensed real-fixture corpus, and exact renderer
output limits. It is likewise a local maintainer check rather than a routine
CI job.

The published crate contains the already-patched vendor tree needed to build,
but deliberately omits the repository maintenance inputs under `scripts/`,
`patches/`, and `upstream/`. Clone the tagged `ManT` repository when reproducing
or changing the patch stack.

### Local vendor patches

The checked-in vendor tree differs from the pinned CVS source subset only by
the 24 ordered patches in `patches/series`. The following group contains
independently reviewable correctness, compatibility, and portability changes;
they are candidates for separate upstream evaluation, not claims of submission
or acceptance:

- `0001-bound-memory-input-utf8.patch` bounds truncated UTF-8 reads at the
  caller-owned buffer end.
- `0002-preserve-unknown-encoding.patch` recognizes common Latin-1 names and
  retains automatic detection for unsupported encoding declarations.
- `0003-preserve-continued-tp-aliases.patch` closes a populated continued
  `.TP`/`.TQ` head before the next declaration, retaining independent terms.
- `0004-retain-already-tagged-mdoc-heads.patch` keeps an explicit `.Tg` on its
  own zero-width node when the following head already owns an automatic ID.
- `0005-keep-ohash-size-unsigned.patch` retains the hash table's unsigned
  size/index domain without lossy MSVC conversions.
- `0006-replace-input-traps.patch` frees superseded `.it` macros and clears
  consumed pointers.
- `0007-free-native-trees-iteratively.patch` frees syntax and equation trees
  without stack growth proportional to depth or sibling count.
- `0008-size-renderer-scratch-buffers.patch` accommodates complete integer
  representations in table-formatting buffers.
- `0009-initialize-renderer-optional-state.patch` initializes guarded
  HTML/terminal temporary state without changing valid output.
- `0010-keep-rfc-url-bytes-unsigned.patch` checks RFC-number bytes without
  signed-character ambiguity.
- `0011-render-direct-layout-sentinels.patch` consumes internal layout
  markers on direct terminal-character paths, including margin characters.
- `0012-libbsd-library-name.patch` adds libbsd's library catalog entry.
- `0013-pandoc-verbatim-fonts.patch` recognizes Pandoc's `\f[V]`,
  `\f[VB]`, and `\f[VI]` fonts.
- `0023-initialize-escape-parser-state.patch` gives optional recursive escape
  state explicit initial values for strict MSVC compilation, retaining the
  existing assignments and diagnostic behavior.
- `0024-retain-executed-tbl-escape-state.patch` retains the actual escape
  character that was active when each tbl row was read, allowing bounded
  source recovery to apply native comment semantics without replaying roff.

The remaining patches implement the synchronous embedding boundary:

- `0014-isolate-parser-session-state.patch` gives remaining mutable parser
  globals thread-local storage and resets unfinished requests between sessions.
- `0015-memory-sources-and-input-budgets.patch` adds memory input and virtual
  source hooks, shares include depth across buffers/files, bounds individual
  loops and aggregate replay to 10,000, and keeps denied or invalid includes
  diagnostic-only.
- `0016-bound-native-parser-depth.patch` bounds mdoc dispatch at 64 levels and
  stops trees beyond 512 parent levels before finalization/validation. The
  rejected mdoc call retains its remaining words as literal content.
- `0017-retain-executed-flow-boundaries.patch` stamps actual allocated nodes
  with a per-document flow generation before validation can remove empty
  paragraphs. `Node::flow_epoch` retains that execution provenance after
  parser release; physical source lines do not substitute for it.
- `0018-deterministic-manual-dates.patch` uses timezone-independent calendar
  dates, fixed English month names, and reentrant current-date conversion.
- `0019-isolate-reference-renderer-state.patch` isolates HTML IDs, table/tab
  state, centered offsets, and page-offset history per instance or thread.
- `0020-capture-deterministic-reference-output.patch` captures bytes in a
  bounded per-call sink and uses explicit UTF-8 encoding with Rust-provided
  Unicode cell widths rather than process locale.
- `0021-portable-memory-renderers.patch` guards unused POSIX/pager interfaces
  in the Windows memory-only formatter build.
- `0022-apply-private-config-to-roff-escapes.patch` applies the private target
  configuration, character policy, and symbol prefix to the new escape unit.

Upstream already provides `MR`, modern standard names, root-element scope
cleanup, and the `tag_put` explicit-tag guard; these are not duplicate local
patches. Equation substitution state is already parser-owned upstream.
Regression tests retain these contracts even when no local hunk is needed.

These patches do not create a separately maintained formatter.
`scripts/sync-vendor --verify` proves the checked-in tree is the pinned source
subset plus exactly this series.

### C shim and Rust AST extensions

The C shim is deliberately separate from `vendor/`: after parsing, it exposes
shallow snapshots of private parser-session structures while Rust performs the
single owned-tree transfer. The snapshots and retained parser never cross the
private synchronous FFI call. In addition to the upstream tree,
`libmandoc-rs` exposes renderer-neutral facts that are already resolved by
libmandoc but unavailable through a public C API:

- normalized mdoc enclosures, list/display/font/author roles, source flags,
  table cells and spans, equations, and validated tags;
- normalized eqn operators plus the common GNU `ldots` macro, which the
  pinned parser otherwise retains as an unexpanded identifier;
- tbl multiline-cell and vertical-continuation flags, including both tbl(7)
  spellings of vertical continuation;
- effective cell content/rule kinds and first-data-row table boundaries,
  retaining native layout precedence and distinguishing `T&` from a new table;
- the pinned native roff-request lookup used by consumers that need to retain
  libmandoc's tbl dispatch boundary without duplicating its request registry;
- structured diagnostics and explicit source/include/compression policy.

These extensions never reinterpret source into `ManT`'s document IR. For
example, semantic presentation of tbl `T{ … T}` cell text belongs to
`mant-codec`, because libmandoc intentionally retains that payload as table
text rather than a nested public syntax tree. The shim can identify native
roff requests, but it does not reinterpret high-level source into `ManT` IR.

## Build requirements and supported targets

The source package vendors libmandoc `cvs-20260911` and compiles it with the `cc`
crate, so a working C compiler is required. Checked configurations are
supplied for Linux/glibc, macOS, and Windows/MSVC. Unix native-file parsing
also requires zlib development headers; Windows builds the memory-only parser
and does not link system zlib; its strict root resolver performs filesystem
transport in Rust. Linux/musl remains rejected until it has a checked
configuration.

`ManT`'s project checks set `LIBMANDOC_RS_DENY_WARNINGS=1` to promote native C
warnings to errors on every supported compiler. MSVC keeps an explicit
five-warning baseline for the pinned upstream sources (`C4100`, `C4146`, `C4200`,
`C4244`, and `C4267`). `C4200` covers its four C99 flexible-array members,
which MSVC diagnoses as an extension even in C11 mode. ManT-owned shim and
compatibility sources promote every baseline family back to errors. This is
opt-in rather than a downstream default so new compiler diagnostics do not
make an existing crate release fail to build for consumers.

## Licensing

The Rust wrapper and C shim are licensed under Apache-2.0.  The vendored
libmandoc source is primarily ISC licensed and includes selected compatibility
files under BSD-2-Clause and BSD-3-Clause terms.  The complete license texts
and upstream attribution are shipped under `LICENSES/` and
`vendor/mandoc-cvs-20260911/LICENSE`.

This crate is not affiliated with the upstream mandoc project.

Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).
