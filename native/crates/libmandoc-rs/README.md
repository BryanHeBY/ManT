# libmandoc-rs

`libmandoc-rs` is a safe Rust ownership boundary around a pinned copy of
[libmandoc](https://mandoc.bsd.lv/).  It parses `man(7)`, `mdoc(7)`, roff,
`tbl(7)`, and `eqn(7)` input into an owned syntax tree, so callers never need
to depend on libmandoc's private C structures or parser lifetime.

## What this crate provides

- A fully owned AST with source locations, macro roles, display/list metadata,
  table cells, equations, and validated same-document tags.
- File parsing with source-relative `.so` includes controlled by the caller.
- Top-level uncompressed, gzip, and zstd manual sources.
- Serialized calls to the upstream parser, whose relevant state is global.

The crate is a parser layer only.  It intentionally does not render terminal
output or HTML, locate system manual pages, interpret application-specific
section models, or run a pager.

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
