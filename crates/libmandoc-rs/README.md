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
- Structured non-fatal diagnostics and typed source/decompression failures.
- Top-level uncompressed, gzip, and zstd manual sources.
- Concurrent parser calls with thread-local upstream and shim state.

The crate is a parser layer only.  It intentionally does not render terminal
output or HTML, locate system manual pages, interpret application-specific
section models, or run a pager.

## Boundary model

```text
plain / gzip / zstd source
          │
          v
Rust transport and policy ──> private C shim ──> vendored libmandoc 1.14.6
          ^                                          │
          └──────── owned ParseReport <──────────────┘
                     ├─ Document syntax tree
                     └─ structured diagnostics
```

The returned tree describes validated roff syntax: macro names, node roles,
fonts, lists, displays, stateful enclosure delimiters, tables, equations,
locations, and tags. It is not
`ManT`'s source-neutral document IR. Consumers that want normalized sections,
semantic entries, typed links, or renderers should use `mant-engine` and
`mant-ir` instead.

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
the memory parser.

`IncludePolicy::Deny` is the default. On Unix, `SourceTree` preserves
libmandoc-compatible lookup beside the source, at the surrounding manual-tree
root, and finally through the process working directory. It is intended for
trusted installed manual trees, not as a containment boundary. `Root(path)` is
the strict policy: it resolves `.so` requests only below a caller-approved
directory, rejects absolute and lexical parent paths, refuses to traverse
symbolic links below that root, and never falls back to the process working
directory. The approved root itself may be a symbolic link. Native C file
inclusion is Unix-only; Windows callers resolve sources first and use the
default memory-only policy.

The vendored parser subset and its include shim make all mutable parse state
thread-local, so independent `Parser` calls may run concurrently. A `Parser`
value is inexpensive immutable configuration; this guarantees parallel calls,
not recursive re-entry through a caller callback on the same OS thread. The
owned node and equation copies stop descending after 256 levels. Pathological
input beyond that defensive cap still returns a successful, finite report but
omits deeper descendants; ordinary manuals remain far below the limit.

Enable the optional `serde` feature to derive `Serialize` and `Deserialize`
for the public AST, parser configuration, reports, diagnostics, and errors.

## Compression contract

For `parse_file`, `Compression::Auto` selects Rust zstd decoding for a `.zst`
suffix. On Windows it also selects Rust gzip decoding for a `.gz` suffix; on
Unix all other paths go through libmandoc's native file reader, including its
gzip detection. Use `Compression::Zstd` to force zstd decoding when a file has
another suffix. For `parse_bytes`, auto mode recognizes zstd magic and plain
input, not gzip; callers must decompress gzip byte streams themselves.
`Compression::Plain` bypasses top-level compression detection. Other
compression formats are not part of this crate's supported contract.

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
concurrent memory and source-tree sessions:

```sh
rustup toolchain install nightly --profile minimal
rustup component add rust-src --toolchain nightly
./scripts/check-thread-safety
./scripts/check-thread-safety --rounds 256
```

The runner supports `x86_64` and `aarch64` Linux/glibc and macOS hosts, uses
an isolated Cargo target directory, stops on the first race, and verifies that
the C archive contains TSAN callbacks. Its tests are ignored by ordinary
`cargo test` because an uninstrumented stress pass cannot establish race
freedom. Windows runs ordinary cross-thread regression tests in CI, but this
TSAN runner does not support Windows.

The published crate contains the already-patched vendor tree needed to build,
but deliberately omits the repository maintenance inputs under `scripts/`,
`patches/`, and `upstream/`. Clone the tagged `ManT` repository when reproducing
or changing the patch stack.

### Local vendor patches

The checked-in vendor tree differs from the official 1.14.6 snapshot only by
the ordered patches in `patches/series`:

- `0001-memory-only-input.patch` adds the buffer-only entry point used on
  Windows and makes denied `.so` requests explicit rather than opening files.
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

Each is a narrow parser or portability correction. They are not a forked
renderer, and `scripts/sync-vendor --verify` proves the checked-in tree is the
official snapshot plus exactly this series.

### C shim and Rust AST extensions

The C shim is deliberately separate from `vendor/`: it copies libmandoc's
private parser-session structures into owned Rust data after parsing. In
addition to the upstream tree, `libmandoc-rs` exposes renderer-neutral facts
that are already resolved by libmandoc but unavailable through a public C API:

- normalized mdoc enclosures, list/display/font/author roles, source flags,
  table cells and spans, equations, and validated tags;
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
and does not link system zlib. Linux/musl remains rejected until it has a
checked configuration.

## Licensing

The Rust wrapper and C shim are licensed under Apache-2.0.  The vendored
libmandoc source is primarily ISC licensed and includes selected compatibility
files under BSD-2-Clause and BSD-3-Clause terms.  The complete license texts
and upstream attribution are shipped under `LICENSES/` and
`vendor/mandoc-1.14.6/LICENSE`.

This crate is not affiliated with the upstream mandoc project.
