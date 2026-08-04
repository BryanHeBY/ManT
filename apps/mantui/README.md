# mantui

`mantui` is ManT's interactive terminal reader for local Unix manual pages
and structured Markdown. It provides a hierarchy-aware sidebar, synchronized
scroll navigation, in-page links, search highlighting, and tldr quick
references.

This npm package contains the Bun/OpenTUI interface only. It intentionally
does not bundle a platform-specific `mant` executable: install the native
document engine separately with Cargo.

## Requirements

- Linux or macOS
- [Bun](https://bun.sh/) 1.3.14 or newer
- Rust 1.88+ and a C toolchain when installing `mant` from crates.io
- Local manual pages and the `man` command for manual topics

## Install

```sh
cargo install mant --version 0.4.0 --locked
bun add --global mantui@0.4.0
```

The npm package is distributed as TypeScript/TSX source and runs with Bun;
it is not a standalone binary and does not require Node.js at runtime.

`mantui` finds the companion `mant` executable through `MANT_PATH` first and
then `PATH`.

## Usage

```sh
mantui git
mantui printf --section 3
mantui README.md
mantui --help
```

Use the separate `mant` command for agents, scripts, Markdown/text/JSON
output, schema discovery, and MCP stdio:

```sh
mant tar --outline
mant tar --explain=--exclude
mant git --format json --compact
mant --mcp
```

For the complete documentation, architecture, protocol reference, and source
build instructions, see the [ManT repository](https://github.com/BryanHeBY/ManT).

## License

Apache-2.0. See `LICENSE` in this package.
