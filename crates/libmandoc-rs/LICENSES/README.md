# License inventory

- `Apache-2.0.txt` covers the Rust wrapper and the ManT-authored C shim.
- `mandoc-1.14.6.txt` is the verbatim upstream license inventory for the
  complete vendored libmandoc 1.14.6 source tree.
- `BSD-3-Clause-Regents.txt` reproduces the terms used by the University of
  California compatibility files. The Windows build compiles `compat_err.c`.
- `BSD-2-Clause-NetBSD.txt` reproduces the terms used by
  `compat_stringlist.c` and `compat_stringlist.h`.
- `BSD-2-Clause-soelim.txt` reproduces the terms used by `soelim.1`.
- `BSD-2-Clause-position-unchanged.txt` preserves the distinct terms used by
  `soelim.c`. This is a descriptive filename, not an SPDX identifier.

The exact file-to-license mapping is recorded in `../THIRD_PARTY_NOTICES.md`.
Every vendored source file also retains its original copyright and permission
header. The published crate excludes the unused `soelim.c` and `soelim.1`
pair. The C file's position-unchanged condition is not yet represented by a
stable SPDX identifier; both files remain unaltered in the repository and
upstream snapshot.
