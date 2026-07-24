# ManT

[![CI](https://github.com/BryanHeBY/ManT/actions/workflows/ci.yml/badge.svg)](https://github.com/BryanHeBY/ManT/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/BryanHeBY/ManT/branch/main/graph/badge.svg)](https://codecov.io/gh/BryanHeBY/ManT)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)

ManT turns dense local Unix manuals into navigable documents for people and
structured knowledge for agents. The same document model also makes local
Markdown navigable and queryable.

Read the complete page in the **`mantui` terminal interface**, or ask the
**`mant` CLI and MCP server** for an outline, one option, a precise excerpt,
or a location-aware search result.

![ManT displaying a tldr quick reference and the structured man(1) page](docs/assets/screenshots/mant-man.png)

## One document model, two workflows

| Tool | Best for | Highlights |
| --- | --- | --- |
| `mantui` | People reading in a terminal | Complete documents, hierarchy-aware sidebar, scroll following, in-page links, search, and tldr quick references |
| `mant` | Agents, scripts, and terminal output | Outlines, targeted excerpts, semantic option explanations, location-aware search, Markdown/text/JSON, schemas, and MCP stdio |

The native `mant` engine parses local `man` and `mdoc` sources with bundled
libmandoc, then exposes one versioned document model to both interfaces. A
system `mandoc` installation is not required. If an installed `tldr` client
has data for a manual topic, ManT places that quick reference before the full
manual.

## Why ManT

- **Structure instead of a flat pager.** Sections, subsections, options, and
  page-local references remain navigable.
- **Reading position stays visible.** The sidebar follows the document after
  scrolling settles, without blocking content movement.
- **Options are semantic nodes.** Agents can explain `--exclude` directly
  instead of retrieving and searching an entire manual.
- **Search results are reusable.** Matches include stable outline nodes and
  Markdown line and column coordinates.
- **Local-first and reproducible.** The primary parser is bundled, the public
  contracts are generated from Rust types, and normal use needs no network
  service.
- **Project docs use the same path.** Local Markdown gains the same outline,
  excerpt, search, TUI, JSON, and MCP capabilities as manuals.

## Install

### Linux release archive

Download the archive for your architecture from the
[latest release](https://github.com/BryanHeBY/ManT/releases/latest), extract
it, and put both executables on `PATH`:

```sh
tar -xzf mant-<version>-linux-<arch>.tar.gz
cd mant-<version>-linux-<arch>
install -Dm755 mantui mant -t ~/.local/bin
```

`mantui` locates its companion CLI through `MANT_PATH` first and then
`PATH`, so keep `mantui` and `mant` together when installing from an archive.
The release archive includes `mant.md` and `mantui.md` for immediate
self-hosted browsing, plus the relevant bundled-parser license. A SHA-256
checksum is published alongside it.

### Build from source

Source builds support Linux and macOS. They require local manual pages and the
`man` command, plus Bun, Rust 1.88+, and a C compiler (GCC on Linux or Clang on
macOS by default).

```sh
bun install
bun run build
PATH="$PWD/dist:$PATH" mantui git
```

The build produces `dist/mantui` and `dist/mant`. For a fast development
loop, use `bun run dev -- git`; it builds and selects the local `mant` binary
automatically.

## Read manuals with `mantui`

```sh
mantui git
mantui printf --section 3
mantui tar
```

The UI always shows the complete manual. Its sidebar mirrors nested sections,
reveals normalized command-line options on demand, follows page-local
references, and synchronizes with the reading position after scrolling
settles. Use `Ctrl+F` or `/` for confirmed search, and `mantui -h` for the
focused interactive command reference.

## Query manuals with `mant`

Start with an outline, then retrieve only the section or option that matters:

```sh
mant git
mant gcc --outline
mant tar --explain=--exclude
mant gcc --node 4.2 --format markdown
```

Heading paths are one-based, while `0` is reserved for an available external
tldr quick reference. The default outline includes semantic options;
`--outline sections` gives callers a smaller section-only tree.

```sh
mant gcc --outline sections
mant tar --node acls --format markdown
mant gcc --node 4.2 --node 4.7 --format json
```

Use the `=` form when an option selector starts with `-`:

```sh
mant tar --explain=--exclude
mant tar --explain exclude
```

`--explain` returns one option, command, or environment-variable entry; use
`--node` for a complete section or tldr content.

Search returns matches with stable Markdown line and column coordinates, plus
the nearest reusable outline path:

```sh
mant tar --search=--acls --context 1
mant gcc --search 'worktree|branch' --regex --case smart
```

Full queries default to clean Markdown. Text and JSON are explicit:

```sh
mant printf --section 3 --format text
mant git --format json --compact
```

For machine integration, discover the versioned JSON Schema from the binary
instead of copying request shapes from documentation:

```sh
mant --schema request
mant --schema all --compact
mant -h
```

Run `mant --update-tldr` to refresh data through the installed client when
available, otherwise through ManT's private cache.

## Read project Markdown through the same model

Use a path for local files or `-` for standard input:

```sh
mantui README.md
mant README.md --outline
mant README.md --node 1
cat guide.md | mant -
```

ManT structures headings, prose, emphasis, code, links, code blocks, lists,
GFM tables, hard breaks, and thematic breaks. Unsupported syntax remains
visible with a diagnostic instead of being silently discarded. An option list
such as ``- `--flag`: description`` becomes the same semantic, addressable
entry used by a manual page.

An exact heading named `TLDR`, `TLDR Quick Reference`, or `Quick Reference`
keeps ManT's distinct quick-reference presentation while remaining part of the
Markdown document. Content before the first heading is addressable as `root`.
The release archive demonstrates this support directly: its `mant.md` and
`mantui.md` manuals are Markdown documents consumed by ManT itself.

## Connect agents over MCP

Run the same native executable as a read-only MCP server over standard input
and output:

```sh
mant --mcp
```

Configure an MCP client with command `mant` and arguments `["--mcp"]`.
`tools/list` exposes generated input and output JSON Schemas for four tools:
`mant_document_outline`, `mant_document_get`, `mant_document_explain`, and
`mant_document_search`. Their shared `target` accepts either a manual topic or
a local Markdown path, and they return the same versioned ManT projections as
the direct CLI. The server has no network transport and no mutation tools; its
standard output is reserved for MCP JSON-RPC, while diagnostics use standard
error.

## Architecture

```text
mantui (Bun / OpenTUI React)
  └─ versioned JSON over stdio → mant
                                  └─ mant-core
                                       ├─ mant-ast
                                       └─ libmandoc-rs
                                            └─ vendored libmandoc + private C shim

MCP client ── stdio JSON-RPC → mant --mcp ──→ mant-core
```

Rust owns source discovery, parsing, the stable AST, tldr integration, and
Markdown/text/JSON output. `libmandoc-rs` exposes an owned, renderer-neutral
parse tree; `mant-core` lowers that tree into ManT's document contract.
TypeScript owns only terminal interaction and presentation after validating the
native response boundary.

## Documentation

- [mant self manual](docs/manuals/mant.md)
- [mantui self manual](docs/manuals/mantui.md)
- [Native architecture and protocol](docs/architecture/native-core.md)
- [Development guide and repository map](docs/development.md)
- [Maintainer release procedure](docs/releasing.md)

## License

ManT is licensed under the [Apache License 2.0](LICENSE). The bundled mandoc
source retains its upstream license.
