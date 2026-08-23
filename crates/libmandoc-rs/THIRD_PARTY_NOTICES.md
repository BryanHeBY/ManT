# Third-party notices

## libmandoc 1.14.6

This crate vendors the libmandoc parser sources from mandoc 1.14.6.  The
vendored tree is preserved under `vendor/mandoc-1.14.6/` and its upstream
license inventory is included verbatim at both
`vendor/mandoc-1.14.6/LICENSE` and `LICENSES/mandoc-1.14.6.txt`.

The vendored sources are locally modified by ManT's ordered patch series for
memory-only input and virtual bundles, parser/renderer compatibility,
portability, bounded output capture, deterministic UTF-8, and independent
thread-local parser and formatter sessions. The crate README lists every
semantic change;
the corresponding tagged repository contains the exact patches and pinned
upstream checksum under `patches/` and `upstream/`. These modifications do not
remove or replace upstream copyright and permission notices.

Most upstream files are distributed under the ISC license.  Some compatibility
files originate elsewhere and retain the following terms:

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
