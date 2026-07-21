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
topic/section requests scanned through `mant-cli --force-libmandoc`. Its header
is the authoritative count for that distribution; the neighbouring README
records the package provenance and observed parser behaviour.

### Known libmandoc limitations found during spot-check

| Page | Issue | Scope |
| ---- | ----- | ----- |
| `ps(1)`, `top(1)`, `free(1)`, `pgrep(1)` (procps-ng) | All `.SH` section titles lost | All procps-ng pages — **fixed** by `sanitize_roff_text` |

Previous scan flagged `.nf`/`.EX` code blocks as H1 heading leaks — this was a
false positive from the grep detection pattern; lines inside fenced code blocks
were matched.  The markdown and text output are correct.

These originate in upstream libmandoc and are currently visible in ManT output.

## Adding or replacing a fixture source

Create or update a distribution-specific directory rather than adding a page
to this root. Its README must record the source repository, retrieval date,
exact package and member paths, upstream and fixture hashes, transformations,
and applicable license text. Update the native topology assertions in the same
commit. These third-party licenses govern only their fixtures; ManT remains
under the repository-level Apache-2.0 license.
