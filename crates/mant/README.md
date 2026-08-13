# mant

`mant` is `ManT`'s local-first documentation command. It turns native man/mdoc
pages and Markdown libraries into one catalog exposed as an interactive TUI,
a deterministic structured CLI, and a read-only stdio MCP server. Every
interface consumes the same normalized document model.

```sh
mant git                              # interactive reader in a terminal
mant gcc --outline                    # semantic hierarchy for an agent
mant tar --explain=--exclude          # retrieve one option directly
mant --input README.md --node 1       # select one Markdown section
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

Source builds require Rust 1.88+ and a C compiler. Linux and macOS additionally
require zlib development headers; Windows uses the checked MSVC memory-parser
configuration without a system zlib.

| Target | Prebuilt archive | Source capabilities |
| --- | --- | --- |
| Linux x64, glibc | Yes | Markdown and native man/mdoc |
| Linux arm64, glibc | Yes | Markdown and native man/mdoc |
| Windows x64, MSVC | Yes | Markdown and native man/mdoc |
| macOS | Source build | Markdown and native man/mdoc |

Targets without a matching archive fall back from `cargo binstall` to a Cargo
source build. Public macOS archives remain disabled until they can be signed
and notarized.

## Interactive reader

An ordinary complete query opens the reader when standard input and output are
terminals. Use `--ui` to require it explicitly:

```sh
mant git
mant --input README.md --ui
```

The reader provides a resizable outline, collapsible sections, semantic option
nodes, settled-scroll following, typed in-page and cross-document links,
back/forward history, full-document search, keyboard and mouse input, and
optional tldr quick references before the full document. `Ctrl+O` opens the
shared Markdown and native-manual catalog. Redirected output stays
deterministic instead of emitting terminal control sequences.

## Structured queries for agents and scripts

Discover the document before selecting only the content that matters:

```sh
mant gcc --outline
mant git --tldr
mant gcc --node 4.2 --format markdown
mant tar --explain=--exclude
mant tar --search=--acls --context 1
```

Output formats include Markdown, text, and JSON. A complete native roff manual
also supports `--format man` for manual-only plain text without tldr content.
Search results carry reusable outline selectors and exact generated-Markdown
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

On Linux, macOS, and Windows, `ManT` indexes raw, gzip, and zstd manual sources
and parses their roff through bundled libmandoc. It does not require a system
`man` or `mandoc` executable at runtime. A leaf manual-page symlink may point
outside its indexed root, but directory symlinks are not traversed and every
`.so` target must remain inside that root. Windows defaults to
`%USERPROFILE%\.local\share\man` and also honors configured manual paths.

Reusable Markdown documents can be registered by relative path below:

- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents`
- macOS: `~/Library/Application Support/ManT/documents`
- Windows: `%APPDATA%\ManT\documents`

Regular Markdown files are discovered recursively with their hierarchy;
symlinks are ignored. Git or direct archive sources configured in sibling
`sources.toml` can be installed with `mant --update-docs` and selected with
`--source`. Removed source tables are reported as orphaned installed data;
preview cleanup with `mant --prune-docs --dry-run` and apply it with
`mant --prune-docs`. An unqualified path or unique component suffix resolves
root documents, sources by
descending priority and ascending source name, then a native manual. See the
complete [document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md).

When compatible local tldr data exists, an unqualified query prepends it as
reserved outline node `0`. `--tldr` selects only that node, while `--manual`
and `--man-section` select only native manual content. Reads prefer installed-client
caches and then `ManT`'s private cache; `mant --update-tldr` updates through an
installed client or the private checkout.

One-off physical input uses `mant --input PATH`; Markdown and plain/gzip/zstd
roff files are supported. Standard input additionally requires
`--input-format markdown|roff`. Manual selectors accept `mant 1 git`,
`mant 'git(1)'`, and `mant manual/1/git`.

## Crate architecture

- `mant-ir` defines the source-neutral in-memory document and quick-reference model.
- `mant-protocol` defines the unified versioned contracts shared by host callbacks, CLI JSON, and MCP.
- `libmandoc-rs` owns the cross-platform libmandoc parser boundary.
- `mant-sources` owns local Markdown discovery and optional source updates.
- `mant-engine` performs document lookup, lowering, projections, search, and output.
- `mant-ui` provides the source-neutral Ratatui frontend.
- `mant` owns command-line, terminal-selection, and MCP process boundaries.

The package also exposes `mant::run` for deterministic single-invocation tests
or embedding with explicit input/output streams, and `mant::run_process` for
the real terminal-sensitive process including MCP. Most library users should
prefer the narrower component crate matching their boundary instead of
embedding the complete command host.

The complete [user manual](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant.md),
[protocol reference](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-protocol.md),
and [release archives](https://github.com/BryanHeBY/ManT/releases) live in the
`ManT` repository.

## License

Apache-2.0. Native builds also contain the separately attributed upstream
mandoc sources distributed by `libmandoc-rs`; downloaded tldr-pages content
is CC BY 4.0 and is attributed when rendered.
