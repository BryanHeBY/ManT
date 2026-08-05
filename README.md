# ManT

[![CI](https://github.com/BryanHeBY/ManT/actions/workflows/ci.yml/badge.svg)](https://github.com/BryanHeBY/ManT/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/BryanHeBY/ManT/branch/main/graph/badge.svg)](https://codecov.io/gh/BryanHeBY/ManT)
[![crates.io](https://img.shields.io/crates/v/mant.svg?logo=rust)](https://crates.io/crates/mant)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)

ManT turns dense local Unix manuals and structurally compatible Markdown into
navigable documents for people and precise, reusable knowledge for agents.
On Windows, the same reader and machine interfaces operate on Markdown
documents while Unix manual parsing remains an explicit Unix capability.
One native `mant` executable provides the full-screen reader, deterministic
Markdown/text/JSON output, generated schemas, and a read-only MCP server.

```sh
mant git                              # interactive reader in a terminal
mant gcc --outline                    # structured outline for a script or agent
mant tar --explain=--exclude          # one semantic option
mant git --format markdown            # deterministic standard output
mant --mcp                            # read-only MCP over stdio
```

![ManT reading its own Markdown manual with a tldr quick reference and semantic outline](docs/assets/screenshots/mant-reader.png)

## One command, two workflows

| Workflow | Selection | Highlights |
| --- | --- | --- |
| Interactive reading | `mant NAME` in a terminal, or `--ui` | Complete document, hierarchy-aware sidebar, scroll following, page-local links, search, mouse input, and tldr quick references |
| Structured queries | Projection options, `--format`, redirection, or `--mcp` | Outlines, excerpts, semantic option explanations, location-aware search, Markdown/text/JSON, generated schemas, and MCP stdio |

A complete query automatically opens the reader only when both standard input
and output are terminals. Redirection remains useful and predictable:

```sh
mant git > git.md
mant git | less
```

Use `--ui` to require the reader or `--format markdown` to require output,
independent of terminal detection.

## Why ManT

- **Structure instead of a flat pager.** Sections, subsections, options, and
  page-local references remain navigable.
- **One interpretation path.** The reader, output renderers, search, schemas,
  and MCP tools consume the same normalized Rust document model.
- **Options are semantic nodes.** Retrieve `--exclude` directly instead of
  searching an entire page.
- **Search results are reusable.** Matches include stable outline nodes and
  generated-Markdown line and column coordinates.
- **Local-first and reproducible.** Unix builds bundle their primary libmandoc
  parser; ordinary use needs no network service or system `man`/`mandoc`
  executable. Windows keeps the same property for Markdown documents.
- **Markdown uses the same model.** Project documentation gains the same
  outline, excerpt, search, TUI, JSON, and MCP capabilities.

## Install

Install a supported prebuilt release with `cargo-binstall`:

```sh
cargo binstall mant
mant git
```

Linux glibc x64/arm64 and Windows MSVC x64 use native release archives. Other
targets fall back to a Cargo source build. To compile explicitly from
crates.io:

```sh
cargo install mant --locked
mant git
```

Building from crates.io requires Rust 1.88+. Unix manual support additionally
requires a C compiler and zlib development headers on Linux or macOS. Windows
builds are pure Rust and provide Markdown, TUI, CLI, tldr, and MCP capabilities
without compiling libmandoc. On Unix, neither a `man` nor a `mandoc` executable
is required at runtime.

### Linux release archive

Download the archive for your architecture from the
[latest release](https://github.com/BryanHeBY/ManT/releases/latest), then
install the executable and its bundled `mant` documentation:

```sh
tar -xzf mant-<version>-linux-<arch>.tar.gz
cd mant-<version>-linux-<arch>
install -Dm755 mant ~/.local/bin/mant
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
install -Dm644 mant.md "$data_home/mant/documents/mant.md"
mant mant
```

Installing `mant.md` is recommended but optional: it makes the complete ManT
manual available as the registered document `mant`, including in the reader,
structured CLI, and MCP document catalog. For a system-wide installation, use
`/usr/local/bin/mant` and `/usr/local/share/mant/documents/mant.md` instead.
User documents take precedence over system documents.

The archive also includes the project README, the Apache-2.0 license, the
bundled mandoc license, and a published SHA-256 checksum.

### Windows release archive

Download `mant-<version>-windows-x64.zip` from the
[latest release](https://github.com/BryanHeBY/ManT/releases/latest), extract
`mant.exe` into a directory on `PATH`, and optionally register the bundled
manual from PowerShell:

```powershell
$documents = Join-Path $env:APPDATA "ManT\documents"
New-Item $documents -ItemType Directory -Force | Out-Null
Copy-Item .\mant.md (Join-Path $documents "mant.md")
mant mant
```

The Windows build reads registered or explicit Markdown, starts the same
interactive reader, and exposes the same output and MCP contracts. It does not
parse Unix man/roff sources; `--manual` and `--section` therefore report that
native manuals are unavailable.

### Build from source

```sh
cargo build --release --locked -p mant
./target/release/mant git
```

See the [development guide](docs/development.md) for full repository checks
and fixture requirements.

## Interactive reader

```sh
mant git
mant printf --section 3
mant README.md
mant tar --ui
```

The sidebar mirrors nested sections and reveals normalized command-line
options on demand. Selecting an entry places it at the top of the content
pane; after scrolling settles, the sidebar follows the first visible section.

- `j` / `k` or arrow keys move through visible nodes.
- `h` / `l` collapse and expand branches.
- `d` / `u` or page keys scroll the document.
- `Ctrl+F` or `/` opens confirmed full-page search.
- `n` and `Shift+N` select the next and previous matches.
- `F10` opens the menu, `?` opens help, and `q` quits.

The mouse can select and fold navigation entries, follow page-local links,
scroll both panes, drag scrollbars, and resize the sidebar.

## Agent, script, and terminal output

Start with an outline and retrieve only the section or option that matters:

```sh
mant gcc --outline
mant gcc --outline sections
mant gcc --node 4.2 --format markdown
mant tar --node acls --format json
mant tar --explain=--exclude
```

Heading paths are one-based. Path `0` and selector `tldr` are reserved for an
available quick reference. The default outline includes semantic options;
`--outline sections` returns only section topology.

Search returns the nearest reusable outline node and exact generated-Markdown
coordinates:

```sh
mant tar --search=--acls --context 1
mant gcc --search 'worktree|branch' --regex --case smart
```

Full output supports Markdown, text, man-style plain text, and JSON:

```sh
mant git --format markdown
mant printf --section 3 --format text
mant git --format json --compact
```

Discover machine contracts from the installed binary rather than copying
request shapes from documentation:

```sh
mant --schema request
mant --schema all --compact
mant --protocol-version
mant --version
```

The [JSON protocol and Schema reference](docs/protocol.md) documents every
versioned request and response projection, normalized AST node, coordinate
rule, and MCP tool.

## Unix manual sources and tldr

On Linux and macOS, ManT indexes raw, gzip, and zstd manual sources directly
and parses their roff through bundled libmandoc. Project-local pages can be exposed through
`MANT_MANPATH` (a complete override) or `MANPATH`:

```sh
mkdir -p ./project-man/man1
cp ./widget.1 ./project-man/man1/widget.1
MANT_MANPATH="$PWD/project-man" mant widget --section 1
```

When compatible local tldr data exists, the reader places it before the full
manual as reserved section `0`. Reads prefer installed-client caches and then
fall back to ManT's private cache below the platform cache directory. Run
`mant --update-tldr` to update through an installed client or that private
checkout.

## Markdown through the same model

Register reusable Markdown under the platform data hierarchy:

```sh
mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents"
cp docs/manuals/mant.md "${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents/mant.md"
mant mant
```

An unqualified name checks the user directory first, then
each `$XDG_DATA_DIRS/mant/documents` root, and finally the native manual index.
On macOS the corresponding roots are
`~/Library/Application Support/ManT/documents` and
`/Library/Application Support/ManT/documents`. Windows uses
`%APPDATA%\ManT\documents` for user documents and
`%PROGRAMDATA%\ManT\documents` for system documents. Nested directories and
file or directory links are discovered recursively. The filename stem remains
the lookup name, so `team/handbook.md` is opened as `mant handbook`;
directories are organizational rather than namespaces. On Unix, an explicit
`--manual` or `--section` request bypasses registered Markdown and selects a
native manual.

Use a path for one-off local files or `-` for non-interactive standard input:

```sh
mant README.md
mant README.md --outline
mant README.md --node 1 --format markdown
cat guide.md | mant -
```

ManT structures headings, prose, emphasis, code, links, code blocks, lists,
GFM tables, hard breaks, and thematic breaks. A complete list such as
``- `--flag`: description`` becomes the same semantic option entry used by a
manual page. Unsupported syntax remains visible with a diagnostic instead of
being silently discarded.

An optional tldr preface at the physical start of a Markdown file uses the
tldr-pages dialect and becomes reserved path `0`. Invisible CommonMark HTML
comments keep the extension markers out of GitHub's rendered page:

```markdown
<!-- mant:tldr:start -->
# tool

> One-line quick reference.

- Run the tool:

`tool {{path/to/input}}`
<!-- mant:tldr:end -->

# Tool
```

The shipped [mant manual](docs/manuals/mant.md) uses this constrained format
and is consumed by ManT itself.

## MCP

Run the same executable as a read-only MCP server:

```sh
mant --mcp
```

Configure the client command as `mant` with arguments `["--mcp"]`. The server
exposes registered-document discovery, outline, content, semantic explanation,
and search tools over stdio. Document tools accept document names rather than
arbitrary filesystem paths; place agent-readable Markdown in the registered
document directories above. It has no network transport or mutation tools;
stdout remains reserved for MCP JSON-RPC and stderr stays silent. Lowering
diagnostics remain available through ordinary CLI JSON queries.

## Architecture

```text
mant
├─ terminal mode ──→ mant-ui (Ratatui)
├─ output mode ────→ Markdown / text / JSON
├─ integration ────→ schemas / request JSON / MCP stdio
└─ mant-core
   ├─ mant-ast
   └─ libmandoc-rs (Unix)
      └─ vendored libmandoc + private C shim
```

Rust owns source discovery, parsing, the stable AST, tldr integration, output,
and terminal presentation. Interactive use passes the in-memory `QueryBundle`
directly to `mant-ui`; external process consumers continue to use the
versioned JSON and MCP boundaries.

## Documentation

- [mant self manual](docs/manuals/mant.md)
- [JSON protocol and Schema reference](docs/protocol.md)
- [Native architecture](docs/architecture/native-core.md)
- [Development guide and repository map](docs/development.md)
- [Maintainer release procedure](docs/releasing.md)

## License

ManT is licensed under the [Apache License 2.0](LICENSE). The bundled mandoc
source retains its upstream license.
