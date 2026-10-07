# Configuration and storage

ManT 0.12 uses the same application-owned directory rules on Linux, macOS and
native Windows. `mant.toml` is optional; it is never found by searching the
working directory or its parents.

| Category | Default | Contents |
| --- | --- | --- |
| Configuration | `~/.config/mant` | `mant.toml`, `sources.toml`, personal `man.conf` |
| Persistent data | `~/.local/share/mant` | `documents/`, `sources/`, `man/` |
| Cache | `~/.cache/mant` | private `tldr-pages/` |
| Installer executable | `~/.local/bin` | `mant` or `mant.exe` |

Home is an absolute `HOME`, with an absolute `USERPROFILE` fallback on Windows.
Windows environment names are ASCII case-insensitive. Native path separators
and path-list separators remain platform-specific.

## Directory precedence

Configuration uses `MANT_CONFIG_HOME`, then `XDG_CONFIG_HOME/mant`, then the
home default. Data and cache use the corresponding `MANT_*_HOME`, then the
corresponding `mant.toml` setting, then `XDG_*_HOME/mant`, then the home default.
ManT-specific variables name **final** directories; XDG variables name shared
bases. Empty directory-category variables are unset. Invalid relative ManT overrides are errors;
relative XDG values are ignored. No category implicitly searches old AppData or
macOS Library storage, merges files from different directories, or jumps back
to the default directory after an override selects a missing file.

```toml
# ~/.config/mant/mant.toml
[paths]
data_home = "~/Documents/mant-data"
cache_home = "~/.cache/mant"

[man]
paths = ["~/manuals", "local manuals"]
discover = true
```

TOML paths name final directories. `~/` expands the selected home; relative
paths use the directory containing `mant.toml`, not the current directory.
Windows can use literal TOML strings such as `data_home = 'D:\ManT Data'`.
There is no `config_home` setting, shell evaluation, arbitrary environment
interpolation, include directive or project-local configuration discovery.
Unknown fields, malformed TOML and unreadable configuration are diagnosed.
Configuration reads are bounded to regular UTF-8 files of at most 1 MiB;
regular-file symlinks are supported, but special files cannot block discovery.

`sources.toml` remains a separate source declaration file under the configuration
root. Its **local Git repo paths still use the data root as their relative base**;
its `path`, `include` and `exclude` fields still select content inside a checkout.

## Manual fallback

`MANT_MANPATH` remains a complete override. `MANPATH` retains path-list override
semantics, including empty components inserting the default sequence. Neither
complete override reads inactive personal or host manual configuration.

With `discover = true` (the default), root precedence is:

1. `mant.toml`'s explicit `man.paths`;
2. personal `man.conf` in the selected configuration directory;
3. platform-native manual discovery and compatibility roots;
4. the application data directory's `man/` root.

Duplicates retain their first occurrence. Fallback is lower-priority **content
discovery**, not merely a test for whether an earlier directory exists: catalogs
and searches include fallback content. Empty or absent `paths` retain this chain.
`discover = false` uses only explicit TOML roots, skipping personal `man.conf`,
host configuration and supplemental roots.

Personal `man.conf` accepts the same bounded subset on all platforms:
`manpath`/`MANPATH`, `MANPATH_MAP`, `MANDATORY_MANPATH` and one-level `MANCONFIG`
fragments. Roots are absolute literal paths; only `MANCONFIG` expands globs.
Double quotes delimit paths containing spaces; backslashes are literal.
`%NAME%` expands process variables once (`%%` is a literal percent), without
executing a shell. Variable names and glob case policy follow the native host.
Invalid entries are omitted and reported by doctor. System man-db and BSD/mandoc
files retain their native dialects and ordering, independently of this personal
configuration. macOS developer/manual discovery is not removed.

## tldr ownership

`MANT_TLDR_DIR` still isolates reads to one directory. Otherwise, the private
fallback is `CACHE_HOME/tldr-pages`. The installed-client discovery gate,
platform/language priority and client-update delegation remain unchanged.
External clients' AppData or Library caches remain compatibility candidates;
they are not relocated by `MANT_CACHE_HOME` and missing platform variables do
not invalidate the private fallback.

## Upgrade and migration

Normal document queries and doctor never create directories, rewrite files,
download resources or migrate storage. Run the one-line installer to upgrade.
For releases 0.12 and newer, the verified target binary resolves settings and
copies legacy configuration and `documents/`, `sources/`, `man/`, `man.d/`
before the installer activates it. Destination configuration wins; conflicting
data aborts the upgrade without overwriting existing content. Original user
configuration and data are retained. Migration is repeatable, rejects active
source updates and links/special files, and is limited to 256 MiB, 100000 tree
entries, 64 levels and 64 MiB per file; larger collections require manual migration.
Installer receipts mark completed layout migration so future upgrades do not
re-import retained originals after the new collection changes.

Relative local Git locations become equivalent absolute locations in the copied
configuration when the data root moves; corresponding verified installed-source
metadata is updated. The original configuration remains a backup. Cache contents
need not move. Explicit installer destinations and custom receipt destinations
are preserved; the installer warns when its custom document destination differs
from runtime discovery. `MANT_DATA_DIR` still means the installer's **document
destination**, whereas `MANT_DATA_HOME` means the application data root.

Internal receipts use `${XDG_STATE_HOME:-~/.local/state}/mant` on all platforms,
with legacy macOS/Windows receipts readable for upgrades and uninstall. No
`MANT_STATE_HOME` option is introduced. Uninstall removes only receipt-owned
files, never user data or the shared `~/.local/bin` PATH entry.
With manual installation disabled, changing the document destination does not
claim same-named files there as installer-owned, even after storage migration.

`mant --doctor` reports effective category paths, their origins, the general and
source configuration locations, active manual findings and legacy source
configuration awaiting migration. Its existing `configPath` field continues
to mean `sources.toml`, not `mant.toml`.
