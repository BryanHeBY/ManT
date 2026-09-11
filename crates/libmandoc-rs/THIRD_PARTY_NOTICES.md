# Third-party notices

## libmandoc cvs-20260911

This crate vendors the mandoc CVS source snapshot pinned to
2026-09-11 08:00:00 UTC. The vendored tree is preserved under
`vendor/mandoc-cvs-20260911/` and its upstream license inventory is included
verbatim at both `vendor/mandoc-cvs-20260911/LICENSE` and
`LICENSES/mandoc-cvs-20260911.txt`. The repository's `upstream/FILES` records
SHA-256 hashes and CVS revisions for all 198 upstream files; `regress/` and CVS
administration are excluded from this source subset.

The vendored sources are locally modified by ManT's ordered patch series for
memory-only input and virtual bundles, parser/renderer compatibility,
portability, bounded output capture, deterministic UTF-8, and independent
thread-local parser and formatter sessions. The crate README lists every
semantic change;
the corresponding tagged repository contains the exact patches and pinned
upstream checksum under `patches/` and `upstream/`. These modifications do not
remove or replace upstream copyright and permission notices.

Most non-trivial upstream files are distributed under the ISC license,
including `roff_escape.c`, `mandoc_dbg.c`, `mandoc_dbg.h`, and
`mandoc_dbg_init.3`. Their complete original notices remain in their headers;
the debug helpers and trivial `test-unveil.c` feature probe are not compiled
into the parser or reference-renderer library. Some compatibility files
originate elsewhere and retain the following terms:

| License | Vendored files |
| --- | --- |
| BSD-3-Clause | `compat_err.c`, `compat_fts.c`, `compat_fts.h`, `compat_getsubopt.c`, `compat_strcasestr.c`, `compat_strsep.c`, `man.1` |
| BSD-2-Clause | `compat_stringlist.c`, `compat_stringlist.h`, `soelim.1` |
| BSD-2-Clause-like, source notice must remain in position and unchanged | `soelim.c` |

The complete reusable terms are under `LICENSES/`; original file headers are
authoritative for copyright years and holders. The Windows/MSVC parser build
compiles `compat_err.c`; the other listed files are retained only as part of
the upstream source snapshot. The published crate excludes the unused
`soelim.c` and `soelim.1` pair because the C file's distinct condition does
not yet have a stable SPDX identifier.
