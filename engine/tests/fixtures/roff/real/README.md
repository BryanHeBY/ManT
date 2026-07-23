# Fixed real-man roff fixtures

This directory is a source catalogue, not a flat collection of files. Each
child directory represents one independently reproducible distribution
snapshot and owns its package provenance, hashes, transformation notes, and
license mapping. The shared `LICENSES/` directory contains the complete texts
required by the third-party manuals.

| Directory | Source snapshot | Stored format | Coverage purpose |
| --- | --- | --- | --- |
| [`archlinux/`](archlinux/README.md) | Arch Linux Archive packages pinned on 2026-07-19 | Original `*.1.gz` package members | Direct gzip input and broad real-man regression coverage |
| [`fedora44/`](fedora44/README.md) | Fedora Linux 44 Everything packages acquired 2026-07-20 | Lossless `*.1.zst` recompressions | zstd decoding and a second current generator corpus |
| [`debian/`](debian/README.md) | Debian sid binary packages acquired 2026-07-21 | Original `*.{1,7}.gz` package members | Third-distribution gzip input and section-7 macro pages |

All fixtures are parsed through ManT's bundled libmandoc. Tests never consult
the host manual database. Fixed compressed roff sources replace the former
renderer-specific HTML snapshots, making parser regressions reproducible across
Linux and macOS while retaining upstream notices embedded in the manuals.

Each distribution directory also contains a `VERIFIED_TOPICS.txt` list of the
topic/section requests scanned through `mant --force-libmandoc`. Its header
is the authoritative count for that distribution; the neighbouring README
records the package provenance and observed parser behaviour.

### `VERIFIED_TOPICS.txt` purpose and principles

These lists exist to show **which upstream packages have actually been exercised**
by the parser, at what breadth, so a reviewer can see coverage at a glance and
reproduce any request with a plain `mant <topic> --section <n>` invocation.

The lists follow these principles:

- **Grouped by originating package.** Topics are attributed to the distribution
  package that ships them (resolved from the downloaded package archive), the
  same way a user reaches a page through `man <topic>`.
- **Verified means no parser crash.** Every listed request was rendered through
  `mant --force-libmandoc` (the strict, no-groff-fallback path) and produced a
  document without a parser failure.
- **C-locale only.** Localized `man/<lang>/` copies are excluded; they are locale
  variants of the same topic and are not separately reachable through the default
  `man -w` lookup.
- **High-volume same-pattern sections are summarised.** When a package emits a
  large, uniform block of pages (for example an OpenSSL `3ssl` API surface or the
  Tcl/Tk `3` entries), that section is recorded as a `# section <n>: <count>
  pages (e.g. …)` line with representative names rather than listing every page.
  This keeps the record compact without losing the essential information —
  which package, how many pages, which section, and what kind of names.

### Known libmandoc / lookup limitations found during spot-check

| Page | Issue | Scope |
| ---- | ----- | ----- |
| `ps(1)`, `top(1)`, `free(1)`, `pgrep(1)` (procps-ng) | All `.SH` section titles lost | All procps-ng pages — **fixed** by `sanitize_roff_text` |
| MariaDB/MySQL `3` pages (Pandoc-generated, `.SS`-only) | Root-level `.SS` subsections dropped → "no readable sections" | ~122 pages — **fixed** by promoting root-level `.SS` to sections |
| Any sectioned lookup (`--section N`) | `man -w <section> -- <topic>` collided with the `--` terminator on man-db | **fixed** by passing the section as `-S <section>` |
| `lastb(1)` and other `.so` stubs with a bare same-directory target | The include resolver stripped the `man#` component, so `.so last.1` looked under the hierarchy root instead of next to the stub | **fixed** by also resolving includes against the unstripped stub directory |

A previous scan flagged `.nf`/`.EX` code blocks as H1 heading leaks — this was a
false positive from the grep detection pattern; lines inside fenced code blocks
were matched.  The markdown and text output are correct.

The `.so` redirects whose targets are absent from a partial corpus (for example
`mariadb-embedded(1)` → a `mariadb(1)` page that was never downloaded) still
fail, but `man(1)` fails on them identically, so ManT stays faithful.

## Adding or replacing a fixture source

Create or update a distribution-specific directory rather than adding a page
to this root. Its README must record the source repository, retrieval date,
exact package and member paths, upstream and fixture hashes, transformations,
and applicable license text. Update the native topology assertions in the same
commit. These third-party licenses govern only their fixtures; ManT remains
under the repository-level Apache-2.0 license.
