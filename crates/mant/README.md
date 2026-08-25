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
shared Markdown and native-manual catalog. The upper-right tab stack keeps
successfully opened documents in first-open order and restores each tab's last
selected semantic node. Redirected output stays deterministic instead of
emitting terminal control sequences.

Visual selections copy automatically, while the Edit menu can copy a complete
semantic node as deterministic text or structurally complete `CommonMark`. Local
sessions prefer the native clipboard and fall back to write-only OSC 52. WSL,
SSH, and VS Code remote sessions prefer OSC 52 so compatible outer terminals
and multiplexers can forward the copy to the user's clipboard. Right-clicking
inside the document copies the retained visual selection again when the mouse
event reaches `ManT`. OSC 52 payloads are limited to 400 KiB before Base64
encoding; native clipboard delivery retains the reader-wide 4 MiB limit. This
terminal protocol has no acknowledgement, so a terminal that disables OSC 52
can silently ignore an emitted copy request.

Help, diagnostics, and tldr output share a terminal-aware colour policy.
Use `--color auto|always|never`; automatic mode respects terminal capability,
`NO_COLOR`, and `TERM=dumb`, while structured formats stay undecorated.

Document discovery follows the same rule: long terminal text is pageable,
while short, redirected, JSON, and explicit `--no-pager` output stays direct.

```sh
mant --list
mant --find git
mant --find git --no-pager
mant --find git --format json
```

## Structured queries for agents and scripts

Discover the document before selecting only the content that matters:

```sh
mant gcc --outline
mant git --tldr
mant gcc --node 4.2 --format markdown
mant tar --explain=--exclude
mant tar --search=--acls --context 1
mant git --search worktree --follow-links
mant --document git --document git-lfs --explain=--work-tree
```

Repeated `--document` values form an ordered query set. `--follow-links` adds typed native-manual and same-source Markdown destinations with bounded breadth-first traversal. Search uses global pagination across the set; explain returns exact per-document matches. Interactive search spans the same pre-resolved set while ordinary document discovery remains global.

Partial document queries default to text and can explicitly select Markdown or
JSON. A complete native roff manual also supports `--format man` for manual-only
plain text without tldr content.
Text projections use semantic ANSI styles on capable terminals and remain plain
under redirection; Markdown, JSON, man, request JSON, and MCP never contain ANSI.
Terminal-bound Markdown masks control characters in dynamic document identities,
while redirected Markdown preserves those data bytes exactly.
Search results carry reusable outline selectors and exact generated-Markdown
coordinates. Machine consumers can discover the authoritative contracts from
the installed executable:

```sh
mant --doctor
mant --doctor --format json --compact
mant --schema request
mant --schema all --compact
mant --protocol-version
```

`mant --doctor` checks the effective local installation without network access,
external processes, or mutations. Text is intended for people; JSON uses the
independent `mant.doctor/v1` report contract, discoverable with
`mant --schema doctor`. Warnings keep a successful exit status, while a broken
promised capability exits with status `1`.

`mant --mcp` exposes read-only discovery, outline, excerpt, explanation, and
search over stdio JSON-RPC. Its five tools return bounded plain text or
`CommonMark` rather than serializing full AST envelopes. MCP stdout contains
protocol messages only; use CLI JSON output to inspect lowering diagnostics.
Every successful tool result begins with a `mant-page` header reporting the
Unicode-scalar character interval and full canonical body size. Clients select
a bounded `maxChars` budget and resume statelessly with `startChar`; semantic
`maxResults` and `maxMatches` limits remain separate from presentation paging.

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

Regular Markdown files are discovered recursively with their hierarchy.
Personal `documents/` may use leaf-file symlinks to regular files, including
external targets; directory and broken links are ignored. Managed source
caches never follow links. Git or direct archive sources configured in sibling
`sources.toml` can be installed with `mant --update-docs` and selected with
`--source`. Removed source tables are reported as orphaned installed data;
preview cleanup with `mant --prune-docs --dry-run` and apply it with
`mant --prune-docs`. An unqualified path or unique component suffix resolves
root documents first. Sources then sort around native manuals at priority zero:
positive sources win, manuals win a zero tie, and non-positive sources are
fallbacks. Omitted source priority defaults to one. See the
complete [document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md).

When compatible local tldr data exists, a combined query that selects a native
section `1` or `8` family page prepends it as reserved outline node `0`.
`--tldr` selects only that node, while `--manual` selects only native manual
content and excludes the quick reference. `--man-section` selects an exact
native full document without disabling a compatible command quick reference.
Explicit tldr lookup compares embedded Markdown quick references using their
document priority;
cached tldr and native manuals share the built-in priority-zero baseline.
Markdown without an embedded quick reference is skipped. Reads prefer
installed-client caches and then `ManT`'s private cache; `mant --update-tldr`
updates through an installed client or the private checkout. If no full
document exists, an ordinary query fails with an explicit `mant NAME --tldr`
hint instead of opening a tldr-only reader.

One-off physical input uses `mant --input PATH`; Markdown and plain/gzip/zstd
roff files are supported. Standard input additionally requires
`--input-format markdown|roff`. Manual selectors accept `mant 1 git`,
`mant 'git(1)'`, and `mant manual/1/git`. A dotted selector such as `git.1`
remains an exact logical name and is never guessed to be a manual shorthand.

## Crate architecture

- `mant-ir` defines the source-neutral in-memory document and quick-reference model.
- `mant-protocol` defines shared query contracts, logical projections, versioned JSON DTOs, and deterministic compact presentation.
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
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## License

Apache-2.0. Native builds also contain the separately attributed upstream
mandoc sources distributed by `libmandoc-rs`; downloaded tldr-pages content
is CC BY 4.0 and is attributed when rendered.
