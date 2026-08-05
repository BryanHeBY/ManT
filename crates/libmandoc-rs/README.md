# libmandoc-rs

`libmandoc-rs` is a safe Rust ownership boundary around a pinned copy of
[libmandoc](https://mandoc.bsd.lv/).  It parses `man(7)`, `mdoc(7)`, roff,
`tbl(7)`, and `eqn(7)` input into an owned syntax tree, so callers never need
to depend on libmandoc's private C structures or parser lifetime.

## What this crate provides

- A fully owned AST with source locations, macro roles, display/list metadata,
  table cells, equations, and validated same-document tags.
- A `Parser` API whose caller-controlled `.so` policy defaults to denial.
- Structured non-fatal diagnostics and typed source/decompression failures.
- Top-level uncompressed, gzip, and zstd manual sources.
- Serialized calls to the upstream parser, whose relevant state is global.

The crate is a parser layer only.  It intentionally does not render terminal
output or HTML, locate system manual pages, interpret application-specific
section models, or run a pager.

## Basic use

```rust,no_run
use libmandoc_rs::{IncludePolicy, ParseOptions, Parser};

let parser = Parser::new(ParseOptions {
    includes: IncludePolicy::SourceTree,
    ..ParseOptions::default()
});
let report = parser.parse_file("/usr/share/man/man1/ls.1.gz")?;

println!("{:?}", report.document.macro_set);
for diagnostic in report.diagnostics {
    eprintln!("{:?}: {}", diagnostic.level, diagnostic.message);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `Parser::parse_bytes` if the caller owns the source transport. Its auto
mode recognizes zstd frames; gzip byte streams should be passed to
`parse_file`, where libmandoc opens them natively.

`IncludePolicy::Deny` is the default. `SourceTree` preserves ordinary manual
tree lookup, while `Root(path)` confines `.so` resolution to a directory the
caller explicitly chooses.

Enable the optional `serde` feature to derive `Serialize` and `Deserialize`
for the public AST, parser configuration, reports, diagnostics, and errors.

## Compression contract

`Compression::Auto` parses ordinary files through libmandoc (including its
native gzip support) and stages `.zst` input through Rust's zstd decoder.
`Compression::Plain` bypasses top-level compression detection, and
`Compression::Zstd` requires a zstd frame. Other compression formats are not
currently part of this crate's supported contract.

## Vendor layering

The vendored C source at `vendor/mandoc-1.14.6/` is derived from the
[official 1.14.6 snapshot](https://mandoc.bsd.lv/snapshots/) with optional
local patches applied. End-user `cargo build` compiles this tree directly;
no network access or external patch tool is required.

Maintainers use `scripts/sync-vendor` to regenerate the vendor tree:

```sh
./scripts/sync-vendor           # download, patch, replace vendor/
./scripts/sync-vendor --verify  # CI: check vendor/ matches upstream + patches
```

The script reads `upstream/SOURCE` for tarball URL and SHA-256, and
`patches/series` for the ordered patch list. Each patch, when present,
has a matching roff reproducer under `tests/`.

## Build requirements and supported targets

The source package vendors libmandoc 1.14.6 and compiles it with the `cc`
crate.  A working C compiler and zlib development library are therefore
required at build time.  Checked configurations are supplied for Linux with
glibc and macOS; Linux/musl and Windows are rejected rather than being built
from an unverified configuration.

## Licensing

The Rust wrapper and C shim are licensed under Apache-2.0.  The vendored
libmandoc source is primarily ISC licensed and includes selected compatibility
files under BSD-2-Clause and BSD-3-Clause terms.  The complete license texts
and upstream attribution are shipped under `LICENSES/` and
`vendor/mandoc-1.14.6/LICENSE`.

This crate is not affiliated with the upstream mandoc project.
