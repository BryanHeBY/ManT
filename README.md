# ManT

[![CI](https://github.com/BryanHeBY/mant/actions/workflows/ci.yml/badge.svg)](https://github.com/BryanHeBY/mant/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)

ManT makes local Unix manual pages easier to explore for people and easier to
query for software. It presents the complete page in a responsive terminal UI,
then exposes the same structured document through a native CLI for agents and
scripts.

![ManT displaying a tldr quick reference and the structured man(1) page](docs/assets/screenshots/mant-man.png)

## Two tools, one document model

| Tool | Best for | Highlights |
| --- | --- | --- |
| `mant` | Reading in a terminal | Complete manual, hierarchy-aware sidebar, in-page links, search, and optional tldr quick reference |
| `mant-cli` | Agents and automation | Markdown, text, JSON, generated schemas, semantic option lookups, and location-aware search |

Both tools parse local `man` and `mdoc` sources with bundled libmandoc. A
system `mandoc` installation is not required. If an installed `tldr` client has
data for the topic, ManT puts that quick reference before the manual.

## Install

### Linux release archive

Download the archive for your architecture from the
[latest release](https://github.com/BryanHeBY/mant/releases/latest), extract
it, and put both executables on `PATH`:

```sh
tar -xzf mant-<version>-linux-<arch>.tar.gz
cd mant-<version>-linux-<arch>
install -Dm755 mant mant-cli -t ~/.local/bin
```

`mant` locates its companion CLI through `MANT_CLI_PATH` first and then
`PATH`, so keep `mant` and `mant-cli` together when installing from an archive.
The release archive includes the relevant bundled-parser license and a SHA-256
checksum is published alongside it.

### Build from source

Source builds support Linux and macOS. They require local manual pages and the
`man` command, plus Bun, Rust 1.85+, and a C compiler (GCC on Linux or Clang on
macOS by default).

```sh
bun install
bun run build
PATH="$PWD/dist:$PATH" mant git
```

The build produces `dist/mant` and `dist/mant-cli`. For a fast development
loop, use `bun run dev -- git`; it builds and selects the local native CLI
automatically.

## Read manuals interactively

```sh
mant git
mant printf --section 3
mant tar
```

The UI always shows the complete manual. Its sidebar mirrors nested sections,
can reveal normalized command-line options on demand, follows page-local
references, and synchronizes with the reading position after scrolling settles.
Use `mant -h` for the focused interactive command reference.

## Query manuals from agents and scripts

Direct content queries default to Markdown:

```sh
mant-cli git
mant-cli printf --section 3 --format text
mant-cli git --format json --compact
```

Discover a document before retrieving only the content you need. Paths are
one-based; `0` is reserved for an available tldr quick reference.

```sh
mant-cli gcc --outline
mant-cli gcc --outline sections
mant-cli tar --node acls --format markdown
mant-cli gcc --node 4.2 --node 4.7 --format json
```

Ask directly about one semantic entry without first walking the outline:

```sh
mant-cli tar --explain=--exclude
mant-cli tar --explain exclude
```

Use the `=` form when the selector starts with `-`. `--explain` returns one
option, command, or environment-variable entry; use `--node` for a whole
section or tldr content.

Search returns matches with stable Markdown line and column coordinates, plus
the nearest reusable outline path:

```sh
mant-cli tar --search=--acls --context 1
mant-cli gcc --search 'worktree|branch' --regex --case smart
```

For machine integration, the versioned JSON Schema is discoverable from the
binary rather than copied from documentation:

```sh
mant-cli --schema request
mant-cli --schema all --compact
mant-cli -h
```

Run `mant-cli --update-tldr` to refresh data through the installed client when
available, otherwise through ManT's private cache.

## Architecture

```text
mant (Bun / OpenTUI React)
  └─ versioned JSON over stdio → mant-cli
                                  └─ mant-core
                                       ├─ mant-ast
                                       └─ libmandoc C shim
```

Rust owns source discovery, parsing, the stable AST, tldr integration, and
Markdown/text/JSON output. TypeScript owns only terminal interaction and
presentation after validating the native response boundary.

## Documentation

- [Native architecture and protocol](docs/architecture/native-core.md)
- [Development guide and repository map](docs/development.md)
- [Maintainer release procedure](docs/releasing.md)

## License

ManT is licensed under the [Apache License 2.0](LICENSE). The bundled mandoc
source retains its upstream license.
