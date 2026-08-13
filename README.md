# ManT

[![CI](https://github.com/BryanHeBY/ManT/actions/workflows/ci.yml/badge.svg)](https://github.com/BryanHeBY/ManT/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/BryanHeBY/ManT/branch/main/graph/badge.svg)](https://codecov.io/gh/BryanHeBY/ManT)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/BryanHeBY/ManT/badge)](https://scorecard.dev/viewer/?uri=github.com/BryanHeBY/ManT)
[![crates.io](https://img.shields.io/crates/v/mant.svg?logo=rust)](https://crates.io/crates/mant)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)

ManT is a local-first documentation reader and query engine. It turns native
man/mdoc pages and Markdown libraries into one navigable catalog for people,
scripts, and agents.

One native `mant` executable provides a full-screen TUI, deterministic
Markdown/text/JSON output, generated schemas, and a read-only MCP server. Every
interface consumes the same typed document model. Bundled libmandoc gives
Linux with glibc, macOS, and Windows the same manual-page parser without
requiring a system `man` or `mandoc` executable at runtime.

![ManT reading its own Markdown manual with a tldr quick reference and semantic outline](docs/assets/screenshots/mant-reader.png)

## Install

Install or update the latest release.

**Unix (Linux with glibc, or macOS)**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.sh | sh
```

**Windows (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.ps1 | iex
```

All options, uninstallation, and alternative methods are in the
[installation guide](docs/installation.md).

**Agent prompt**

```text
Read https://raw.githubusercontent.com/BryanHeBY/ManT/main/docs/installation.md
and install or update the latest ManT release for this system. Use its
recommended user-scoped method, verify the installation, and report any PATH
change still needed.
```

## One binary, one model, three interfaces

| Interface | Entry point | Designed for |
| --- | --- | --- |
| Interactive TUI | `mant NAME` in a terminal, or `--ui` | Hierarchical reading, document discovery, typed links, history, search, mouse input, and tldr quick references |
| Structured CLI | Projection options, `--format`, or redirection | Outlines, excerpts, semantic explanations, location-aware search, and stable Markdown/text/JSON |
| Read-only MCP | `mant --mcp` | Local discovery and focused document retrieval for agents over stdio |

A complete query automatically opens the reader only when both standard input
and output are terminals. Redirection remains useful and predictable:

```sh
mant git > git.md
mant git | less
```

Use `--ui` to require the reader or `--format markdown` to require output,
independent of terminal detection.

## Why ManT

- **Navigate a documentation library, not just one page.** A single catalog
  covers personal Markdown, installed sources, and native manual sections;
  typed links and bounded back/forward history connect them.
- **Address structure directly.** Sections, options, commands, variables, and
  environment variables are semantic nodes, so `--exclude` can be retrieved
  without searching or copying the complete page.
- **Get the same interpretation everywhere.** The TUI, CLI renderers, search,
  generated schemas, and MCP tools consume one normalized Rust document model.
- **Keep automation predictable.** Outlines and excerpts use explicit selectors;
  search results include reusable nodes and generated-Markdown coordinates.
- **Stay local-first.** Ordinary reading and querying need no network service,
  and the bundled parser avoids a runtime dependency on host manual tools.
- **Treat Markdown as documentation, not a second-class fallback.** It receives
  the same hierarchy, links, semantic entries, search, output, and agent access
  as native manuals.

## Interactive reader

```sh
mant git
mant --input README.md
mant tar --ui
```

The Outline sidebar mirrors nested document sections and reveals normalized
options, commands, variables, and environment variables on demand. Selecting
a node places its target at the top of the content pane; after scrolling
settles, the outline follows the first visible document node.

- `j` / `k` or arrow keys move through visible nodes.
- `h` / `l` collapse and expand branches.
- `d` / `u` or page keys scroll the document.
- `Ctrl+O` opens a live finder for registered Markdown and native manuals.
- `Alt+Left` / `Alt+Right` move backward and forward through document jumps.
- `Ctrl+F` or `/` opens confirmed full-page search.
- `n` and `Shift+N` select the next and previous matches.
- `F10` opens the menu, `?` opens help, and `q` quits.

The mouse can select and fold outline nodes, follow underlined in-page,
cross-document, and web/email links, scroll both panes, drag scrollbars, and
resize the Outline sidebar. Markdown links stay inside their registered source; man
and mdoc references select an exact manual section.

## Structured discovery and queries

Discover installed Markdown and native manuals without opening each document:

```sh
mant --list
mant --find process
mant --find '^git' --regex --kind manual --format json
```

`--list` exposes one tree rooted at `documents/`, `sources/<source>/`, and
`manual/<section>/`. `--find` emits stable tab-separated canonical paths by
default, making it suitable for filtering and shell pipelines. Literal
discovery ranks exact paths or leaf names first, then component suffixes,
prefixes, and other substring matches; stable path order breaks ties. A query
containing `/` also matches the complete canonical path.

Start with an outline and retrieve only the section or option that matters:

```sh
mant gcc --outline
mant git --tldr
mant gcc --node 4.2 --format markdown
mant tar --node acls --format json
mant tar --explain=--exclude
```

Heading paths are one-based. Path `0` and selector `tldr` are reserved for an
available quick reference. `--tldr` selects that node across embedded Markdown
and cached tldr candidates, and permits a quick reference even when no full
document exists. On a color terminal
its default output uses the same semantic styles as the TUI; pipes, `NO_COLOR`,
and `TERM=dumb` receive plain text.
`--color always|never` overrides detection, while an explicit `--format`
continues to select Markdown, text, or JSON.

Search returns the nearest reusable outline node and exact generated-Markdown
coordinates:

```sh
mant tar --search=--acls --context 1
mant gcc --search 'worktree|branch' --regex --case smart
```

Full output supports Markdown, text, and JSON. Native roff manuals additionally
support `--format man`, which emits manual-only plain text without tldr content:

```sh
mant git --format markdown
mant --input README.md --format text
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

The [structured protocol and Schema reference](docs/manuals/mant-protocol.md) documents every
versioned request and response projection, coordinate rule, and MCP tool. The
separate [IR reference](docs/manuals/mant-ir.md) describes the richer
in-process model from which those wire types are projected.

## Build a local documentation library

`mant --list` presents one logical tree regardless of where a document came
from:

```text
documents/                  personal Markdown
sources/<source>/           installed Markdown collections
manual/<section>/           native man and mdoc pages
```

Exact catalog paths are unambiguous. Short selectors use root documents first;
configured sources then sort around native manuals at priority `0`. Positive
sources win, manuals win a zero tie, and negative sources act as fallbacks.
Unique component suffixes make a deep path such as `languages/en/tool`
convenient without hiding collisions.

### Native manuals

ManT indexes raw, gzip, and zstd pages from traditional `man<section>/`
directories and flat roots containing files such as `widget.1`. Project-local
collections can use `MANT_MANPATH` as a complete override:

```sh
mkdir -p ./project-man/man1
cp ./widget.1 ./project-man/man1/widget.1
MANT_MANPATH="$PWD/project-man" mant widget --manual
```

The same index works on Linux with glibc, macOS, and Windows. Logical queries
accept `mant 1 git`, `mant 'git(1)'`, `mant git --man-section 1`, and the canonical
path `mant manual/1/git`. Manual aliases and parser I/O remain bounded to their
indexed collection; the complete lookup and `.so` policy is documented in the
[mant manual](docs/manuals/mant.md).

### Markdown collections

Personal `.md` and `.markdown` files below ManT's `documents/` directory keep
their extension-free relative hierarchy. Configured Git repositories and
direct archive URLs are installed in the sibling managed `sources/` directory,
without mixing their files into the personal tree:

```toml
[team]
repo = "https://github.com/example/cli-docs.git"
branch = "main"
path = "manuals"
priority = 10
```

Run `mant --update-docs` to install or update configured sources and
`mant --prune-docs --dry-run` before explicitly removing orphaned source data.
Windows selectors try an exact documented executable name before following
`PATHEXT`, so packages can retain canonical names such as `tool.exe.md` while
`mant tool` remains convenient. The [document-source guide](docs/sources.md)
defines platform paths, archive configuration, selection, precedence, and
transactional update behavior.

Personal `documents/` may use leaf-file symlinks to regular files, including
targets outside that tree; broken links and directory symlinks are ignored.
Installed source caches never follow links, and source acquisition rejects
selected Git or archive links so packages remain portable across platforms.

Markdown headings, prose, code, links, lists, tables, and selected semantic
definition lists enter the same model as native manuals. Unsupported syntax
remains visible with a diagnostic instead of being silently discarded. The
bundled [mant-markdown(7)](docs/manuals/mant-markdown.md) and
[mant-roff(7)](docs/manuals/mant-roff.md) manuals define the exact input
contracts; every bundled manual is also a self-hosted ManT document.

### One-off input and quick references

Physical files are deliberately separate from logical catalog selectors:

```sh
mant --input README.md
mant --input /usr/share/man/man1/git.1.gz --outline
cat guide.md | mant --input - --input-format markdown
cat widget.1 | mant --input - --input-format roff
```

When compatible local tldr data exists, an ordinary query places its quick
reference before the full document as reserved node `0`. `mant git --tldr`
selects only that presentation, while `--manual` and `--man-section` select only
native manual content. For `--tldr`, a Markdown candidate participates only
when it actually embeds a quick reference: personal documents win, positive
source priorities precede the cached tldr baseline at `0`, and zero or negative
sources follow it. `--source NAME` restricts this lookup to the selected
Markdown source. A cached tldr entry does not make a missing ordinary document
query succeed: ManT reports the failed lookup and suggests the explicit
`mant NAME --tldr` command instead. ManT reads installed-client caches or its
private cache, which `mant --update-tldr` can update. Markdown authors may also
embed a document-owned quick reference using the format described in the
[mant manual](docs/manuals/mant.md).

## MCP

Run the same executable as a read-only MCP server:

```sh
mant --mcp
```

Configure the client command as `mant` with arguments `["--mcp"]`. The server
exposes local discovery, outline, content, semantic explanation, and search
tools over stdio. Each call reads the files currently visible in `documents/`,
configured installed sources, and native manual paths. MCP has no update,
network, or mutation tool and does not promise a session snapshot across
calls. Lowering diagnostics remain available through ordinary CLI JSON queries.

## Architecture

![ManT architecture: source adapters enter mant-engine around the mant-ir semantic center; the TUI consumes document IR directly and uses mant-protocol host DTOs, while CLI and request JSON and MCP share the same versioned contracts](docs/assets/architecture.svg)

`mant-ir` is the semantic center nested inside the `mant-engine` execution
layer. Interactive use passes its in-memory `ResolvedContent` directly to
`mant-ui`, and human renderers operate on the same model. Structured host and
process interactions share the versioned `mant-protocol` contract layer;
catalog callbacks, CLI JSON, request JSON, and MCP therefore use the same
logical identities and projections. Git and archive updates are an optional native CLI capability
layered on `mant-sources`, not part of document reads or MCP.

## Documentation

- [Installation methods and platform requirements](docs/installation.md)
- [mant(1) command manual](docs/manuals/mant.md)
- [Document source configuration and updates](docs/sources.md)
- [mant-protocol(5) structured integration contract](docs/manuals/mant-protocol.md)
- [mant-ir(7) normalized document model](docs/manuals/mant-ir.md)
- [mant-markdown(7) supported Markdown](docs/manuals/mant-markdown.md)
- [mant-roff(7) native manual compatibility](docs/manuals/mant-roff.md)
- [Native engine and crate boundaries](docs/architecture/native-engine.md)
- [Development guide and repository map](docs/development.md)
- [Maintainer release procedure](docs/releasing.md)

## License

ManT-authored work is licensed under the [Apache License 2.0](LICENSE).
Rust dependencies, bundled parser sources, cached tldr-pages content,
real-world test fixtures, and screenshot fonts retain their upstream terms;
upstream tldr pages are CC BY 4.0 and are attributed at render time. See the
[third-party notice map](THIRD_PARTY_NOTICES.md). Native releases include a
CycloneDX SBOM and signed GitHub provenance/SBOM attestations.
Please report vulnerabilities through the private process in the
[security policy](SECURITY.md).
