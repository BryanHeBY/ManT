# ManT

[![CI](https://github.com/BryanHeBY/mant/actions/workflows/ci.yml/badge.svg)](https://github.com/BryanHeBY/mant/actions/workflows/ci.yml) [![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)

ManT is a structured terminal UI for local Unix manual pages. It combines a
native document engine with an OpenTUI React interface and can show an optional
tldr quick reference before the full manual.

![ManT displaying a tldr quick reference and the structured man(1) page](docs/assets/screenshots/mant-man.png)

The project installs two separate executables:

- `mant` is the interactive TUI.
- `mant-cli` is the Rust query tool for agents, scripts, JSON, and Markdown.

`mant` does not embed or extract `mant-cli`. It resolves `MANT_CLI_PATH` first,
then looks for `mant-cli` on `PATH`.

## Requirements

- Linux or macOS with local manual pages and the `man` command
- [Bun](https://bun.sh/) for the TUI and project scripts
- Rust 1.85 or newer
- GCC on Linux or Clang on macOS; set `CC` to override the default

The libmandoc source is vendored and compiled as part of the Rust workspace.
A system `mandoc` installation is not required. An installed `tldr` client is
optional.

## Develop from a new clone

```sh
bun install
bun run dev -- git
```

The development command performs an incremental release build of `mant-cli`,
stages it under `native/bin`, sets `MANT_CLI_PATH`, and then starts the Bun TUI.
An optional manual section can be selected with:

```sh
bun run dev -- printf --section 3
```

## Build

```sh
bun run build
```

The build runs TypeScript checks, Rust formatting/tests/Clippy, Bun tests, and
packaged smoke tests. It produces current-platform executables in `dist/`:

```text
dist/mant
dist/mant-cli
```

Install both files into the same directory on `PATH`. To exercise an uninstalled
build directly, either add `dist` to `PATH` or select the CLI explicitly:

```sh
PATH="$PWD/dist:$PATH" ./dist/mant git
MANT_CLI_PATH="$PWD/dist/mant-cli" ./dist/mant git
```

Official tagged releases currently provide paired, standalone executables for
Linux on x64 and arm64. macOS remains supported when building from source, but
prebuilt macOS archives are withheld until they can be Developer ID-signed and
notarized. Each Linux archive is accompanied by a SHA-256 checksum and includes
the licenses needed by the bundled libmandoc parser. A release tag must match
the version in both `package.json` and `native/Cargo.toml`:

```sh
git tag v0.2.0
git push origin v0.2.0
```

GitHub Actions builds each archive on its target architecture rather than
cross-compiling the Rust/C native core. Linux x64 uses Bun's baseline target so
the TUI does not require AVX2. After both builds pass, the workflow creates
a draft GitHub Release with generated notes and every asset. Review or edit the
notes in GitHub, then publish the draft manually; publishing does not rebuild
the binaries.

## Agent and script usage

Direct queries default to Markdown:

```sh
mant-cli git
mant-cli printf --section 3 --format markdown
```

`--format` is the sole output selector and accepts `markdown`, `text`, or
`json`. Apart from the topic itself, every public argument uses a `--long`
option name.

Discover the complete query outline, then request only the content needed by a
human or agent. Manual paths are one-based; an available tldr quick reference
uses the reserved path `0`. `--node` also accepts the ID printed in brackets:

```sh
mant-cli gcc --outline
mant-cli gcc --outline --format json
mant-cli tar --outline options
mant-cli tar --node acls --format markdown
mant-cli gcc --node 0 --format markdown
mant-cli gcc --node tldr --format text
mant-cli gcc --node 4.2 --format markdown
mant-cli gcc --node options-4 --format text
mant-cli gcc --node 4.2 --node 4.7 --format json
```

Selecting a node includes all of its child sections. Repeated and overlapping
selections are deduplicated and emitted in source order. `--section` continues
to select the manual volume (for example `1` or `3p`), not an outline node.
The optional `options` outline detail adds normalized command-line option
entries beneath their sections; each can be selected by its printed path, ID,
or alias without loading an unrelated section into an agent's context.

Search the same canonical document without scraping terminal output:

```sh
mant-cli tar --search=--acls
mant-cli gcc --search warning --context 1
mant-cli git --search 'worktree|branch' --regex --case smart
mant-cli tar --search=--acls --format json --compact
```

`--search` (also `--grep`) defaults to a case-insensitive literal match over
visible document text. `--regex`, `--word`, `--case`, `--scope`, `--context`,
`--limit`, and `--offset` make matching and pagination explicit. Each result
contains a one-based line and column in ManT's complete Markdown rendering and
the nearest section or option path accepted directly by `--node`. Markdown
coordinates therefore stay stable across text, JSON, and Markdown reports.

When a platform unexpectedly falls back to groff, force the native parser to
inspect its unmodified result and diagnostics:

```sh
mant --force-libmandoc tar
mant-cli tar --force-libmandoc --format json
```

This diagnostic switch disables groff fallback for that invocation. It is a
strict local execution policy: a failed or empty libmandoc parse is reported
even when a cached tldr page exists, and recoverable parser findings are
printed on standard error. It is deliberately not a field in the versioned
request JSON contract.

Use the versioned JSON contract for structured consumers:

```sh
mant-cli git --format json
mant-cli git --format json --compact
mant-cli git --format text
mant-cli --protocol-version
mant-cli --schema request
mant-cli --schema all --compact
```

`--schema` accepts `request`, `query`, `outline`, `excerpt`, `search`, or `all`. These
Draft 2020-12 JSON Schemas are generated from the same Rust types used by the
runtime, so agents can discover the exact input and output contracts without
copying declarations from documentation. A versioned stdio request looks like:

```json
{
  "schema": "mant.request/v2",
  "topic": "printf",
  "section": "3",
  "view": { "kind": "full" }
}
```

The same input boundary can request projections without inventing CLI syntax:

```json
{"schema":"mant.request/v2","topic":"tar","view":{"kind":"outline","detail":"options"}}
{"schema":"mant.request/v2","topic":"tar","view":{"kind":"excerpt","nodes":["acls"]}}
{"schema":"mant.request/v2","topic":"tar","view":{"kind":"search","pattern":"--acls","limit":20}}
```

Update tldr data through its installed client when available, otherwise through
ManT's private cache:

```sh
mant-cli --update-tldr
```

## Architecture

```text
mant (Bun/OpenTUI React)
  └─ stdio JSON → mant-cli
                    └─ mant-core
                         ├─ mant-ast
                         └─ mant-mandoc-sys → private libmandoc C shim
```

Rust owns source discovery, man/mdoc/groff parsing, tldr behavior, the stable
AST, and JSON/Markdown serialization. TypeScript validates `mant.query/v2` at
the process boundary and owns only terminal interaction and presentation. See
[`docs/architecture/native-core.md`](docs/architecture/native-core.md) for the
contract and fallback policy.

## Focused checks

```sh
bun test
bun run lint
bun run rust:test
bun run rust:lint
```

## License

ManT is licensed under the [Apache License 2.0](LICENSE).
