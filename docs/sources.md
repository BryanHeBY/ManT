# Document Sources

ManT can update small collections of Markdown documentation from an ordinary
Git repository or a directly downloadable archive. This is deliberately a
narrow document installer, not a general package manager: a source has either
one Git branch or one archive URL, and only Markdown files are installed.
Upstream inputs may use nested directories; every installed source is flattened
into one private directory for lookup.

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
└── documents/
    ├── personal.md
    └── sources/
        └── team/
            ├── .mant-source.toml
            └── tool.md
```

Files directly inside `documents/` are managed by the user. Each immediate
directory below `documents/sources/` is managed by `mant --update-docs`; do not
edit it by hand because a later update replaces the complete directory.

Every discoverable Markdown document remains below `documents/`. Only `.md`
and `.markdown` files directly inside that root or one installed source
directory are discoverable. Lookup uses the filename stem. Other directories,
extensions, and symbolic links are ignored.

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

[release]
url = "https://example.com/cli-docs/latest/docs.zip"
path = "docs"
exclude = ["drafts"]
```

Fields are:

| Field | Required | Meaning |
| --- | --- | --- |
| `repo` | For Git | HTTPS/SSH Git URL or local repository path; relative paths use the ManT data root |
| `branch` | For Git | Exact branch checked through `refs/heads/<branch>` |
| `url` | For archive | Direct HTTPS URL of a ZIP, tar, tar.gz/tgz, or tar.zst/tzst archive |
| `path` | No | Directory inside the checkout; defaults to `.` |
| `include` | No | Exact relative files or directory subtrees below `path` |
| `exclude` | No | Exact relative files or directory subtrees removed after inclusion |
| `priority` | No | Signed integer used for fallback; defaults to `0` |

Configure either `url`, or both `repo` and `branch`; they cannot be combined.
The archive URL is the complete artifact identity, so a fixed release belongs
in the URL itself and there is no separate provider, release, tag, or archive
format field. Format detection uses the downloaded bytes rather than the URL
suffix.

Source names use lowercase ASCII letters, digits, `-`, and `_`, beginning with
a letter or digit. Paths are trimmed, relative, use `/` on every platform,
cannot contain `.`/`..` components (`path = "."` is the one root shorthand),
and do not accept glob syntax. A selector
matches either one exact path or everything below that
directory. When `include` is absent or empty, every Markdown file below `path`
is eligible; `exclude` always wins.

The source scan is recursive because upstream inputs may organize their files
in subdirectories. `path = "."` selects the root of either a checkout or an
extracted archive. Installation is flat. If two selected files
would have the same public filename stem (compared case-insensitively for
cross-platform safety), the source update fails and the
previous installation remains in place.
Selecting no Markdown files is also an error, which protects an existing source
from being replaced after a mistyped `path`, `include`, or `exclude` value.

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

For a Git source, ManT reads the branch head with `git ls-remote`. It skips an
unchanged source or performs a depth-one, single-branch clone without tags,
local hardlinks, or submodule initialization. Git transport is restricted to
HTTPS, SSH, and local paths; remote-helper syntax and other protocols are
rejected. The `git` executable must be installed and available on `PATH` for
Git-backed sources.

For an archive source, ManT sends saved `ETag` and `Last-Modified` validators
when available. A `304 Not Modified` response avoids downloading and
extracting the artifact. Otherwise ManT streams the response to a bounded
temporary file and records its SHA-256 digest as the revision; the digest also
detects unchanged content when a server provides no validators. Redirects stay
on HTTPS and are limited to five hops. Archive downloads and ZIP, tar,
tar+gzip, and tar+zstd extraction are built in and do not require Git or
external archive commands.

Both paths then select only regular `.md` and `.markdown` files, flatten them,
check public-name collisions, and write `.mant-source.toml`. The normalized
configuration fingerprint excludes `priority`, so a priority-only change
takes effect immediately without reinstalling identical files. The installed
directory is replaced only after staging succeeds.

Archive processing is intentionally bounded: downloads are limited to 64 MiB,
archives to 20,000 entries and 256 MiB of declared expanded regular-file data,
individual Markdown files to 16 MiB, selected Markdown files to 10,000, and
paths to 32 components. Absolute, parent-relative, non-UTF-8, duplicate, link,
and special-file entries are rejected. These checks apply before activation,
so malformed or hostile input leaves the previous source installed.

An update lock prevents two native CLI updates from writing the source store
at once. A failed source leaves its prior installed directory untouched. If a
process is killed and leaves `.update.lock`, verify that no update is running
and remove only that lock file before retrying.

## Lookup

For `mant tool`, Markdown lookup is:

1. `documents/tool.md` or `documents/tool.markdown`;
2. configured sources by descending `priority`, then source name in ascending
   bytewise order;
3. the native manual index.

Within one directory, `.md` wins over `.markdown` for the same stem. Shadowed
source candidates remain discoverable. Select exactly one configured source
with:

```sh
mant tool --source team
```

Windows packages should keep executable suffixes in document stems, for
example `tool.exe.md`. An extensionless query first checks `tool`, then
`tool` plus each `PATHEXT` suffix in order; an explicitly suffixed query is
exact. The behavior is Windows-wide rather than PowerShell-specific, and
non-Windows platforms do not omit executable suffixes.

`--source` cannot be combined with `--section` or `--manual`, and an explicit
source never falls back to root documents or native manuals. The JSON request
contract uses the same optional `source` field on a `document` input.

## MCP behavior

`mant --mcp` only reads local state visible at the time of each tool call:
`sources.toml`, installed Markdown files, and native manual paths. It has no
source-update tool, does not invoke Git, download archives, or use the network,
and does not promise a fixed snapshot across multiple calls. If a native CLI
update completes between calls, a later MCP call may see the new files.
