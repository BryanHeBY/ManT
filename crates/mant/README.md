# mant

`mant` is ManT's complete native document command: an interactive terminal
reader for people, a deterministic structured CLI for scripts and agents, and
a read-only stdio MCP server. Every surface consumes the same normalized
document model.

```sh
mant git                              # interactive reader in a terminal
mant gcc --outline                    # semantic hierarchy for an agent
mant tar --explain=--exclude          # retrieve one option directly
mant README.md --node 1               # select one Markdown section
mant git --format json --compact      # deterministic machine output
mant --mcp                            # read-only MCP over stdio
```

## Install

Download a supported prebuilt GitHub Release through
[`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo binstall mant
```

Or compile from crates.io:

```sh
cargo install mant --locked
```

Source builds require Rust 1.88+. Linux and macOS manual support also requires
a C compiler and zlib development headers; Windows builds are pure Rust.

| Target | Prebuilt archive | Source capabilities |
| --- | --- | --- |
| Linux x64, glibc | Yes | Markdown and native man/mdoc |
| Linux arm64, glibc | Yes | Markdown and native man/mdoc |
| Windows x64, MSVC | Yes | Markdown |
| macOS | Source build | Markdown and native man/mdoc |

Targets without a matching archive fall back from `cargo binstall` to a Cargo
source build. Public macOS archives remain disabled until they can be signed
and notarized.

## Interactive reader

An ordinary complete query opens the reader when standard input and output are
terminals. Use `--ui` to require it explicitly:

```sh
mant git
mant README.md --ui
```

The reader provides a resizable outline, collapsible sections, semantic option
nodes, settled-scroll following, page-local links, full-document search,
keyboard and mouse input, and optional tldr quick references before the full
document. Redirected output stays deterministic instead of emitting terminal
control sequences.

## Structured queries for agents and scripts

Discover the document before selecting only the content that matters:

```sh
mant gcc --outline
mant gcc --node 4.2 --format markdown
mant tar --explain=--exclude
mant tar --search=--acls --context 1
```

Output formats include Markdown, text, man-style plain text, and JSON. Search
results carry reusable outline selectors and exact generated-Markdown
coordinates. Machine consumers can discover the authoritative contracts from
the installed executable:

```sh
mant --schema request
mant --schema all --compact
mant --protocol-version
```

`mant --mcp` exposes the same read-only document discovery, outline, excerpt,
explanation, and search capabilities over stdio JSON-RPC. MCP stdout contains
protocol messages only; use CLI JSON output to inspect lowering diagnostics.

## Document sources

On Linux and macOS, ManT indexes raw, gzip, and zstd manual sources and parses
their roff through bundled libmandoc. It does not require a system `man` or
`mandoc` executable at runtime. Windows intentionally omits this Unix source
family.

Reusable Markdown documents can be registered by filename below:

- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents`
- macOS: `~/Library/Application Support/ManT/documents`
- Windows user: `%APPDATA%\ManT\documents`
- Windows system: `%PROGRAMDATA%\ManT\documents`

Nested directories and symlinks are discovered recursively. An unqualified
`mant NAME` resolves registered Markdown before a native manual with the same
name. Explicit Markdown paths and `mant -` standard input remain available for
one-off documents; `--manual` bypasses registered Markdown on Unix.

When compatible local tldr data exists, ManT prepends it as reserved outline
node `0`. Reads prefer installed-client caches and then ManT's private cache;
`mant --update-tldr` updates through an installed client or the private
checkout.

## Crate architecture

- `mant-ast` defines the versioned document and query contracts.
- `libmandoc-rs` owns the Unix libmandoc parser boundary.
- `mant-core` performs source lookup, lowering, projections, search, and output.
- `mant-ui` provides the source-neutral Ratatui frontend.
- `mant` owns command-line, terminal-selection, and MCP process boundaries.

The complete [user manual](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant.md),
[protocol reference](https://github.com/BryanHeBY/ManT/blob/main/docs/protocol.md),
and [release archives](https://github.com/BryanHeBY/ManT/releases) live in the
ManT repository.

## License

Apache-2.0. Unix builds also include the upstream mandoc licenses distributed
by `libmandoc-rs`.
