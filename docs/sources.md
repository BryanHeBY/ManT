# Document Sources

ManT can update small collections of Markdown documentation from ordinary Git
repositories. This is deliberately a narrow document installer, not a general
package manager: it checks one branch, clones one shallow revision, installs
only Markdown files, and never initializes submodules.

## Layout

ManT uses one per-user data root:

| Platform | Data root |
| --- | --- |
| Linux | `${XDG_DATA_HOME:-$HOME/.local/share}/mant` |
| macOS | `~/Library/Application Support/ManT` |
| Windows | `%APPDATA%\ManT` |

The layout is fixed:

```text
mant/
├── sources.toml
├── documents/
│   └── personal.md
└── sources/
    └── team/
        ├── .mant-source.toml
        └── tool.md
```

`documents/` is for files managed directly by the user. Each immediate
directory below `sources/` is managed by `mant --update-docs`; do not edit it
by hand because a later update replaces the complete directory.

Only `.md` and `.markdown` files directly inside `documents/` or one installed
source directory are discoverable. Lookup uses the filename stem. Directories,
other extensions, and symbolic links are ignored.

## Configuration

Each top-level table in `sources.toml` is a source. There is no `source.` table
prefix. The UTF-8 file is limited to 1 MiB:

```toml
[team]
repo = "https://github.com/example/cli-docs.git"
branch = "main"
path = "docs/commands"
include = ["stable", "overview.md"]
exclude = ["stable/internal", "stable/draft.md"]
priority = 20

[community]
repo = "https://github.com/example/community-docs.git"
branch = "release"
priority = 0
```

Fields are:

| Field | Required | Meaning |
| --- | --- | --- |
| `repo` | Yes | Git URL or local repository path |
| `branch` | Yes | Exact branch checked through `refs/heads/<branch>` |
| `path` | No | Directory inside the checkout; defaults to `.` |
| `include` | No | Exact relative files or directory subtrees below `path` |
| `exclude` | No | Exact relative files or directory subtrees removed after inclusion |
| `priority` | No | Signed integer used for fallback; defaults to `0` |

Source names use lowercase ASCII letters, digits, `-`, and `_`, beginning with
a letter or digit. Paths are trimmed, relative, use `/` on every platform,
cannot contain `.`/`..` components (`path = "."` is the one root shorthand),
and do not accept glob syntax. A selector
matches either one exact path or everything below that
directory. When `include` is absent or empty, every Markdown file below `path`
is eligible; `exclude` always wins.

The repository scan is recursive because upstream repositories may organize
their inputs in subdirectories. Installation is flat. If two selected files
would have the same public filename stem (compared case-insensitively for
cross-platform safety), the source update fails and the
previous installation remains in place.

## Updating

Run:

```sh
mant --update-docs
```

The result is stable JSON identified by `mant.sources-update/v1`. Add
`--compact` for one-line JSON. Each source is reported as `updated`,
`unchanged`, or `failed`; successful sources are kept even when another source
fails, and the process exits with status `1` after printing the complete report
if any source failed.

For each source ManT:

1. Reads the branch head with `git ls-remote`.
2. Compares the commit and normalized configuration fingerprint with local
   `.mant-source.toml` metadata. Priority-only changes take effect immediately
   and do not reinstall identical files.
3. Skips the clone when both are unchanged.
4. Otherwise performs a depth-one, single-branch clone without tags or
   submodule initialization.
5. Selects only regular `.md` and `.markdown` files, flattens them, checks
   public-name collisions, and writes new metadata.
6. Replaces that source directory only after the complete staging result is
   ready.

An update lock prevents two native CLI updates from writing the source store
at once. A failed source leaves its prior installed directory untouched. If a
process is killed and leaves `.update.lock`, verify that no update is running
and remove only that lock file before retrying.

## Lookup

For `mant tool`, Markdown lookup is:

1. `documents/tool.md` or `documents/tool.markdown`;
2. configured sources by descending `priority`, then source name in ascending
   bytewise order;
3. the native Unix manual index.

Within one directory, `.md` wins over `.markdown` for the same stem. Shadowed
source candidates remain discoverable. Select exactly one configured source
with:

```sh
mant tool --source team
```

`--source` cannot be combined with `--section` or `--manual`, and an explicit
source never falls back to root documents or native manuals. The JSON request
contract uses the same optional `source` field on a `document` input.

## MCP behavior

`mant --mcp` only reads local state visible at the time of each tool call:
`sources.toml`, installed `.mant-source.toml` metadata, Markdown files, and
native manual paths. It has no source-update tool, does not invoke Git or use
the network, and does not promise a fixed snapshot across multiple calls. If a
native CLI update completes between calls, a later MCP call may see the new
files.
