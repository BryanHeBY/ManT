# Document Sources

ManT can update small collections of Markdown documentation from an ordinary
Git repository or a directly downloadable archive. This is deliberately a
narrow document installer, not a general package manager: a source has either
one Git branch or one archive URL, and only Markdown files are installed.
Upstream inputs may use nested directories. Every installed source preserves
that relative hierarchy in its private lookup directory.

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
│   ├── personal.md
│   └── languages/
│       └── zh-CN/
│           └── tool.md
└── sources/
    └── team/
        ├── .mant-source.toml
        └── guides/
            └── tool.md
```

Everything below `documents/` is managed by the user. Each immediate directory
below the sibling `sources/` store is managed by `mant --update-docs`; do not
edit it by hand because a later update replaces the complete directory.

Every discoverable Markdown document belongs to either the personal
`documents/` tree or one installed source below `sources/`. Regular `.md` and
`.markdown` files are discovered recursively and addressed by their
extension-free path relative to that origin. The
personal tree accepts an explicitly named leaf-file symlink when its target is
a regular file, including a target outside `documents/`; the link path supplies
the logical identity. Broken links and directory symlinks are ignored. Managed
source caches never follow links. A registry snapshot is bounded to 32
directory levels and 10,000 logical documents per origin; exceeding either
limit fails discovery instead of returning a partial tree.

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
| `priority` | No | Signed integer relative to native manuals at `0`; defaults to `1` |

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
extracted archive. Installation preserves each selected file's path below that
root. Two files may share a leaf name in different directories. If `.md` and
`.markdown` would produce the same logical path, `.md` wins; paths that differ
only by ASCII case are rejected for cross-platform safety. A failed source
leaves the previous installation in place.
Selecting no Markdown files is also an error, which protects an existing source
from being replaced after a mistyped `path`, `include`, or `exclude` value.

## Diagnosing local state

Run `mant --doctor` to inspect the effective data paths, source configuration,
installed source identities, registered documents, native manual index, bundled
libmandoc, optional Git requirement, and tldr caches. The command is offline and
read-only: it does not create directories or locks, invoke Git, download
archives, update caches, or remove orphaned sources. It reports the existing
maintenance command to use when action is required.

Use `mant --doctor --format json` for the versioned `mant.doctor/v1` report.
Physical paths are included deliberately because this is a local diagnostic
interface; doctor is not exposed through MCP.

Installed-source metadata records a document count. Doctor compares that count
with the currently materialized Markdown files, and update reacquires a source
when they differ. This detects ordinary missing-file damage but is not a path
manifest or content-integrity hash: a same-count rename or replacement is
outside this check. A locally consistent source is not necessarily at the
remote branch head: doctor remains offline and explicitly reports that remote
freshness was not checked. Run `mant --update-docs` to compare Git sources with
their remote branch or refresh archive validators.

## Updating

Run:

```sh
mant --update-docs
```

The result is stable JSON identified by `mant.sources-update/v2`. Add
`--compact` for one-line JSON. Each source is reported as `updated`,
`unchanged`, or `failed`; successful sources are kept even when another source
fails, and the process exits with status `1` after printing the complete report
if any source failed. The `orphaned` array separately reports immediate entries
below `sources/` whose names are absent from the current
configuration. Ordinary updates never delete them.

For a Git source, ManT reads the branch head with `git ls-remote`. It skips an
unchanged source or performs a depth-one, single-branch clone without tags, an
initial checkout, local hardlinks, or submodule initialization. Unix clones
request a blobless partial checkout before materializing only the configured
`path`. Windows downloads the complete single-commit tree because some Git for
Windows versions do not reliably hydrate a filtered no-checkout clone during
pathspec checkout. Git transport is restricted to HTTPS, SSH, and local paths;
remote-helper syntax and other protocols are rejected. The `git` executable
must be installed and available on `PATH` for Git-backed sources.

For an archive source, ManT sends saved `ETag` and `Last-Modified` validators
when available. A `304 Not Modified` response avoids downloading and
extracting the artifact. Otherwise ManT streams the response to a bounded
temporary file and records its SHA-256 digest as the revision; the digest also
detects unchanged content when a server provides no validators. Redirects stay
on HTTPS and are limited to five hops. Archive downloads and ZIP, tar,
tar+gzip, and tar+zstd extraction are built in and do not require Git or
external archive commands.

Both paths then select only regular `.md` and `.markdown` files, preserve their
relative paths, check logical-path collisions, and write `.mant-source.toml`. The normalized
configuration fingerprint excludes `priority`, so a priority-only change
takes effect immediately without reinstalling identical files. The installed
directory is replaced only after staging succeeds.

Source snapshots are deliberately self-contained. Archive symbolic links and
hard links are rejected. For Git, ManT reads tree modes before installation and
rejects a selected Markdown entry with mode `120000`, even on Windows where Git
may check that entry out as a regular file containing only its target. A
configured `path` also cannot traverse a Git link. Unrelated and unselected Git
links do not affect the source. Source authors should publish regular Markdown
files instead of filesystem aliases.

Acquired snapshots are intentionally bounded to 20,000 entries, 256 MiB of
materialized regular-file data, 16 MiB per selected Markdown file, 10,000
Markdown files, and paths 32 components deep. Archive downloads have an
additional 64 MiB compressed-size limit, and archive entry metadata is charged
before extraction. Git command output is bounded too; Git checkout contents
are measured before staging. Windows intentionally receives every blob in the
depth-one snapshot, and a Unix server may ignore Git's partial-clone filter, so
unlike the streaming archive path this cannot strictly cap transient Git pack
traffic or storage before checkout validation. Absolute,
parent-relative, non-UTF-8, duplicate, link, and special archive entries are
rejected. These checks apply before activation, so malformed or hostile input
leaves the previous source installed.

An update lock prevents two native CLI updates from writing the source store
at once. A failed source leaves its prior installed directory untouched. If a
process is killed and leaves `.update.lock`, verify that no update is running
and remove only that lock file before retrying.

## Removing sources

First remove or rename the source table in `sources.toml`. The source becomes
unqueryable immediately, while its updater-owned installed directory remains
available for an explicit cleanup decision. Preview every exact target with:

```sh
mant --prune-docs --dry-run
```

The stable `mant.sources-prune/v1` JSON report uses `would-remove` for verified
targets. Apply the same discovery and validation rules with:

```sh
mant --prune-docs
```

Successful targets are reported as `removed`. A candidate is removable only
when it is a direct child of `sources/`, has a valid source name, is
a real directory rather than a symbolic link, and contains a regular
`.mant-source.toml` whose recorded source matches the directory name. Invalid
names, links, special files, missing or mismatched metadata, permission
failures, and candidates that change during cleanup are retained and reported
as `refused` or `failed`; either action produces exit status `1` after the
complete JSON report is printed. The sibling `documents/` tree is entirely
outside the prune boundary.

If interruption leaves a `.prune-*` directory, later maintenance reports it
as an incomplete transaction and never deletes it automatically. Inspect its
contents and remove it manually only after confirming that the corresponding
configured or installed source is intact.

Update and prune share `.update.lock`, so they cannot mutate the source store
concurrently. Cleanup renames a verified target inside the same source store
before removing it, preventing a partially addressed source from remaining
under its public name.

## Lookup

For `mant tool`, document lookup is:

1. an exact or unique component-suffix path below personal `documents/`;
2. configured sources with positive `priority`, in descending priority and
   then ascending bytewise source-name order;
3. the native manual index, treated as priority `0`;
4. configured sources with priority `0` or below, again in descending priority
   and ascending source-name order.

Native manuals win ties at priority `0`. Because the configured-source default
is `1`, an existing source that omits `priority` continues to override a manual
with the same selector. Set cross-platform or fallback documentation to `0` or
a negative value when the installed native page should win.

`mant NAME --tldr` applies the same order to quick-reference content. Personal
documents and configured sources participate only when the matching Markdown
file contains an embedded tldr preface. Cached tldr occupies the built-in
priority-zero position, so positive sources override it and zero or negative
sources follow it. `--source NAME --tldr` restricts resolution to that source.

Exact relative paths win before component suffixes. If one origin contains
both `languages/en/tool.md` and `languages/zh/tool.md`, the selector `tool` is
ambiguous and ManT lists both paths instead of guessing. Complete catalog
selectors (`documents/languages/en/tool`, `sources/team/guides/tool`, and
`manual/1/tool`) address one candidate directly. Within one logical path,
`.md` wins over `.markdown`. Shadowed source candidates remain discoverable.
Select exactly one configured source with:

```sh
mant tool --source team
```

Windows packages should keep executable suffixes in document stems, for
example `tool.exe.md`. An extensionless query first checks `tool`, then
`tool` plus each `PATHEXT` suffix in order; an explicitly suffixed query is
exact. The behavior is Windows-wide rather than PowerShell-specific, and
non-Windows platforms do not omit executable suffixes.

`--source` cannot be combined with `--man-section` or `--manual`, and an explicit
source never falls back to root documents or native manuals. The JSON request
contract uses the same optional `source` field on a `document` input. Native
manual selection instead uses `manualSection`; it never means a heading inside
the loaded document.

## MCP behavior

`mant --mcp` only reads local state visible at the time of each tool call:
`sources.toml`, installed Markdown files, and native manual paths. It has no
source-update tool, does not invoke Git, download archives, or use the network,
and does not promise a fixed snapshot across multiple calls. If a native CLI
update completes between calls, a later MCP call may see the new files.
