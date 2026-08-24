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
Rust transport and policy ──> private C shim ──> vendored libmandoc 1.14.6
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
semantic entries, typed links, or renderers should use `mant-engine` and
`mant-ir` instead.

All bundled parser and compatibility definitions are compiled under the
`mant_vendored_*` namespace; only the private `mant_mandoc_*` Rust/shim bridge
remains separately named. Linking this crate therefore does not inject generic
symbols such as `strlcpy`, `ohash_init`, or `mparse_alloc` into a consumer's
native symbol namespace. Cargo's `links = "libmandoc_rs"` key still permits
only one `libmandoc-rs` version in a dependency graph. A breaking pre-1.0
upgrade must consequently be coordinated across every dependent crate rather
than relying on parallel `0.x` versions.

The shim retains the completed native parser only during the synchronous FFI
transfer. Each native node and table cell is exposed as a shallow borrowed
snapshot and copied directly into the public Rust tree; no borrowed pointer
escapes the call and no intermediate heap-owned C AST is materialized. The
private parser handle is destroyed on the calling thread before `Parser`
returns, while the returned report remains fully owned and freely movable.

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
ordinary manuals remain far below both limits.

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

The vendored C source at `vendor/mandoc-1.14.6/` is derived from the
[official 1.14.6 snapshot](https://mandoc.bsd.lv/snapshots/) with ordered
local patches applied. End-user `cargo build` compiles this tree directly;
no network access or external patch tool is required.

The local thread-safety patch moves each mutable parser-global slot in the
compiled libmandoc subset into static thread-local storage. It uses C11 TLS on
Linux and macOS, and `__declspec(thread)` on Windows/MSVC; macOS's native
process-global program-name slot is initialized once before concurrent parses.
Date-only metadata is converted without process-global timezone state, while
the special current-date form uses the platform's reentrant local-time API.

From a `ManT` repository checkout, maintainers use `scripts/sync-vendor` to
regenerate the vendor tree while working in `crates/libmandoc-rs/`:

```sh
./scripts/sync-vendor           # download, patch, replace vendor/
./scripts/sync-vendor --verify  # CI: check vendor/ matches upstream + patches
```

The vendor synchronizer reads `upstream/SOURCE` for the tarball URL and
SHA-256, and `patches/series` for the ordered patch list. `--verify`
reconstructs the tree from those inputs and compares it with `vendor/`.
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

The checked-in vendor tree differs from the official 1.14.6 snapshot only by
the ordered patches in `patches/series`:

- `0001-memory-only-input.patch` adds the buffer-only entry point used on
  Windows and makes `.so` requests without an explicit bundle or strict root
  resolver fail rather than opening files implicitly.
- `0002-man-mr.patch` recognizes the modern man(7) `MR` reference macro.
- `0003-pandoc-verbatim-fonts.patch` recognizes Pandoc's `\f[V]`, `\f[VB]`,
  and `\f[VI]` font escapes.
- `0004-libbsd-library-name.patch` adds libbsd to libmandoc's recognized
  library-name catalog.
- `0005-modern-standards.patch` adds POSIX.1-2024 and C23 standard aliases.
- `0006-thread-local-parser-state.patch` gives independent parser calls
  isolated mutable libmandoc state without a process-wide lock.
- `0007-thread-safe-date-conversion.patch` makes ordinary manual dates
  timezone-independent and uses reentrant conversion for the special current
  date form.
- `0008-bounded-while-expansion.patch` limits each roff `.while` loop to
  10,000 iterations, retains the finite prefix, and emits libmandoc's existing
  infinite-loop diagnostic instead of allowing hostile input to parse forever.
- `0018-bound-aggregate-while-expansion.patch` shares a 10,000-replay budget
  across every `.while` statement and user-macro call in one parser session,
  preventing individually bounded loops from multiplying into unbounded work.
- `0009-preserve-unknown-encoding.patch` recognizes common Latin-1 declaration
  spellings and retains automatic UTF-8/Latin-1 detection when a `coding:`
  declaration names an unsupported charset, avoiding irreversible `?`
  replacement for bytes the bundled converter cannot interpret explicitly.
- `0010-reset-roff-session-state.patch` clears unfinished input-trap and
  centering state at every parser-session boundary, preventing a page ending
  with an armed request from carrying dangling native pointers into the next
  parse in a long-lived process.
- `0011-bound-memory-input-utf8.patch` falls back to Latin-1 for a truncated
  UTF-8 sequence at a caller-owned buffer boundary instead of reading past the
  supplied memory.
- `0012-replace-input-traps.patch` frees the superseded `.it` trap macro when
  a page replaces it, preventing repeated trap declarations from accumulating
  memory in a long-lived parser process.
- `0013-memory-source-bundles.patch` lets the memory parser recursively read
  `.so` targets through the shim's per-call source hook. That hook serves a
  virtual bundle or the strict Windows root resolver, and finalizes only after
  the outermost memory source with the same recursion bound as files.
- `0019-share-input-recursion-depth.patch` keeps memory buffers and native
  files in one parser-owned include-depth counter, so a native `.so` target
  cannot finalize the document while its caller-owned outer buffer still has
  content to parse.
- `0020-bound-mdoc-macro-recursion.patch` routes all mdoc macro dispatch
  through a parser-owned 64-level nesting budget, retaining the rejected macro
  and remaining words as visible text instead of overflowing the C stack.
- `0014-isolate-renderer-output.patch` routes ASCII and HTML bytes into the
  shim's bounded per-call sink, makes formatter ID/tab state thread-local,
  releases per-call tab storage, and widens small integer-format buffers to
  their complete representable sizes.
- `0015-deterministic-utf8-rendering.patch` replaces libmandoc's process-locale
  UTF-8 setup with explicit sink encoding and caller-supplied Unicode cell
  widths, giving Linux, macOS, and Windows the same locale-independent path.
- `0016-portable-memory-renderers.patch` removes unused POSIX header and pager
  process types from the Windows memory-only formatter build while retaining
  the complete upstream interfaces for native Unix builds.
- `0025-keep-ohash-size-unsigned.patch` keeps the compatibility hash table's
  bounded size and probe indices in their public `unsigned int` domain,
  avoiding lossy `size_t` round trips on 64-bit MSVC.
- `0017-preserve-continued-tp-aliases.patch` closes a populated `.TP`/`.TQ`
  head before a following tagged paragraph when the tag ends in `\\c`, so
  legacy GNU pages retain consecutive long and short option aliases instead
  of deleting the first tag as a broken next-line scope.
- `0021-isolate-terminal-renderer-state.patch` moves table borders, centered
  table offsets, and roff page-offset history from C statics into each terminal
  renderer, preventing both cross-thread races and same-thread document leaks.
- `0022-keep-denied-includes-diagnostic-only.patch` keeps an embedded `.so`
  rejected by parser policy observable as a diagnostic without synthesizing
  the rejected target path into visible document prose.
- `0023-keep-invalid-includes-diagnostic-only.patch` treats an absolute or
  parent-traversing embedded `.so` as a diagnostic-only rejected request,
  retaining surrounding content without inserting the invalid path into
  visible document prose.
- `0024-deterministic-manual-dates.patch` formats validated manual dates with
  fixed English month names instead of consulting the process `LC_TIME`.

Each is a narrow parser, renderer-boundary, or portability correction. They do
not create a separately maintained formatter. `scripts/sync-vendor --verify`
proves the checked-in tree is the official snapshot plus exactly this series.

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
- structured diagnostics and explicit source/include/compression policy.

These extensions never reinterpret source into `ManT`'s document IR. For
example, semantic reconstruction of roff requests inside tbl `T{ … T}` cells
belongs to `mant-engine`, because libmandoc intentionally retains that payload
as table text rather than a nested public syntax tree.

## Build requirements and supported targets

The source package vendors libmandoc 1.14.6 and compiles it with the `cc`
crate, so a working C compiler is required. Checked configurations are
supplied for Linux/glibc, macOS, and Windows/MSVC. Unix native-file parsing
also requires zlib development headers; Windows builds the memory-only parser
and does not link system zlib; its strict root resolver performs filesystem
transport in Rust. Linux/musl remains rejected until it has a checked
configuration.

`ManT`'s project checks set `LIBMANDOC_RS_DENY_WARNINGS=1` to promote native C
warnings to errors on every supported compiler. This is opt-in rather than a
downstream default so new compiler diagnostics do not make an existing crate
release fail to build for consumers.

## Licensing

The Rust wrapper and C shim are licensed under Apache-2.0.  The vendored
libmandoc source is primarily ISC licensed and includes selected compatibility
files under BSD-2-Clause and BSD-3-Clause terms.  The complete license texts
and upstream attribution are shipped under `LICENSES/` and
`vendor/mandoc-1.14.6/LICENSE`.

This crate is not affiliated with the upstream mandoc project.

Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).
