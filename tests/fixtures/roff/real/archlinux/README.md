# Arch Linux fixtures

These fixtures contain roff manual bytes extracted from the immutable Arch
Linux Archive packages listed below. The original `*.1.gz` fixtures were
recorded in ManT on 2026-07-19. The `gawk` and `rsync` package references were
verified on 2026-07-23 after those fixtures were added: their gzip members were
decompressed without changing the roff bytes and recompressed as zstd to retain
coverage of ManT's in-process zstd decoder.

`archive_entry_stat.3` was added on 2026-08-19 from Arch's libarchive package.
It stores the exact decompressed roff bytes so the declaration-unit regression
remains directly inspectable; its source hash matches the host-audit ledger.

`expand_number.3bsd` was added on 2026-08-23 from Arch's libbsd package after
a mandoc comparison exposed a missing comma between multiple operands of one
mdoc `Fa` invocation. It likewise stores the exact decompressed source and
retains the complete page-specific BSD-2-Clause notice.

They form the primary real-man corpus for section topology, definition lists,
preformatted blocks, inline fonts, navigation, and source-markup regressions.
The neighbouring Fedora corpus supplies independently packaged generator
output.

| Fixture | Upstream and Arch package | Package member | Storage | Fixture license | Fixture SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `ls.1.gz` | [GNU coreutils], [Arch `coreutils` 9.11-2] | `usr/share/man/man1/ls.1.gz` | Original member | [GPL-3.0-or-later] | `091e614c945887862980212abe697c63b946fbb4d189c741ad47c5dd71bd4ea0` |
| `git.1.gz` | [Git], [Arch `git` 2.55.0-1] | `usr/share/man/man1/git.1.gz` | Original member | [GPL-2.0-only] | `8b58cbf77d1eb0ca9efcea2a98790574dcf3c2f76d02ce08531af1e931a926ed` |
| `gcc.1.gz` | [GCC], [Arch `gcc` 16.1.1+r346+g4e03491b401d-4] | `usr/share/man/man1/gcc.1.gz` | Original member | [GFDL-1.3-invariants-or-later] | `8a0bbfaaa5b05a8fcefc6d4741530d09abcfd95b26f8947e9aecbce68cb75b23` |
| `clang.1.gz` | [LLVM Clang], [Arch `clang` 22.1.8-1] | `usr/share/man/man1/clang.1.gz` | Original member | [Apache-2.0 WITH LLVM-exception] | `313398b1f95b070d7a807ea8cc2d28403b0e25159960b9fa9ce90d820bff5bed` |
| `tar.1.gz` | [GNU tar], [Arch `tar` 1.35-2] | `usr/share/man/man1/tar.1.gz` | Original member | [GPL-3.0-or-later] | `dfeee239e4bbed1d271c0902c0fce79e5844c4d4778deae3e8d9c9995341c726` |
| `gawk.1.zst` | [GNU gawk], [Arch `gawk` 5.2.0-1] | `usr/share/man/man1/gawk.1.gz` | Lossless zstd recompression | [gawk manual-page permission] | `942ba8de74fb6ef25f683a935edb54424ef61404fc9ddc5b47ebf822c23e2a50` |
| `rsync.1.zst` | [Rsync], [Arch `rsync` 3.4.3-1] | `usr/share/man/man1/rsync.1.gz` | Lossless zstd recompression | [GPL-3.0-or-later] | `cb2becd7d2448b4f27fc28e36ea377d2667e9f814b9295b0b5ce45c06d0495a2` |
| `sh.1p.gz` | [POSIX sh], [Arch `man-pages` 6.18-1] | `usr/share/man/man1p/sh.1p.gz` | Original member | [POSIX manual notice] | `464243a5da22f585063698896dc115ab81ef10950a3d75e3f00d3d3874b3785e` |
| `archive_entry_stat.3` | [libarchive], [Arch `libarchive` 3.8.9-1] | `usr/share/man/man3/archive_entry_stat.3.gz` | Exact decompressed source | [BSD-2-Clause] | `06311cf3566f167804ef732defd0e5d1375dd68ceb001713be666de69e58581b` |
| `expand_number.3bsd` | [libbsd], [Arch `libbsd` 0.12.2-2] | `usr/share/man/man3/expand_number.3bsd.gz` | Exact decompressed source | [BSD-2-Clause] | `4e0d2bd2af63de49f6c55ce96ae07b52a6c2214d837f4c33eec47699ca19de03` |

The two recompressed fixtures preserve these exact decompressed roff hashes:

- `gawk.1`: `d28fc0d5bfdc08f85faaa6267b14223520967f9fdf0730550f12fee880b2ca31`
- `rsync.1`: `12417d699e494cd5154195df53762f0043e2ffe3997634c4e5f4afc209f87d45`
- `sh.1p`: `8caa52a52fcb46e6e4e38105408dac290b66865cf712f6897d81bfa438f16b2d`

The corresponding immutable package archives have these SHA-256 values:

- `gawk-5.2.0-1-x86_64.pkg.tar.zst`: `dd6a14cb65eec0754eb0d77a373bc685cff2776133007251e35593a3de8045f6`
- `rsync-3.4.3-1-x86_64.pkg.tar.zst`: `f2ad0dcc4d7022cb7f04c4da716be067b93a95fc246f2c0259cb2dbb880684e5`
- `man-pages-6.18-1-any.pkg.tar.zst`: `f03bbc27c6c14aed6c009a4780e618cf57c0eb9cdca390a3c4eacc600d197ba3`
- `libbsd-0.12.2-2-x86_64.pkg.tar.zst`: `e26194849786b0202828a348be3f4b90d410604cd7d48f113a4301584a49895a`

The GCC manual embeds its own GFDL invariant sections, front-cover text, and
back-cover text. Those page-specific notices remain in `gcc.1.gz`; the shared
[`GFDL` text](../LICENSES/GFDL-1.3-invariants-or-later.txt) supplies the
complete license it references. [`LLVM.txt`](../LICENSES/LLVM.txt) is the full
license text shipped with the matching Arch Clang package, including the Apache
License 2.0, LLVM exception, and legacy LLVM notice. The gawk page's own
copying permission is retained in its `COPYING PERMISSIONS` section and
transcribed in [`GAWK-MANPAGE.txt`](../LICENSES/GAWK-MANPAGE.txt).
The POSIX shell page is redistributed under the IEEE and The Open Group
permission shipped by Arch's man-pages package; its required notice is copied
verbatim to [`POSIX-COPYRIGHT.txt`](../LICENSES/POSIX-COPYRIGHT.txt).
The libarchive and libbsd pages retain their complete BSD-2-Clause notices at
the start of each fixture.

## Reproducing a fixture

Download the exact archive package and extract the existing compressed member.
For the original gzip fixtures, do not recompress it. For example:

```sh
curl -LO https://archive.archlinux.org/packages/c/coreutils/coreutils-9.11-2-x86_64.pkg.tar.zst
bsdtar -xOf coreutils-9.11-2-x86_64.pkg.tar.zst \
  usr/share/man/man1/ls.1.gz > ls.1.gz
sha256sum ls.1.gz
```

For the two zstd fixtures, decompress the package member and recompress only
the unchanged roff bytes:

```sh
curl -LO https://archive.archlinux.org/packages/r/rsync/rsync-3.4.3-1-x86_64.pkg.tar.zst
bsdtar -xOf rsync-3.4.3-1-x86_64.pkg.tar.zst \
  usr/share/man/man1/rsync.1.gz | gzip -dc > rsync.1
zstd -19 -f -o rsync.1.zst rsync.1
sha256sum rsync.1 rsync.1.zst
```

When replacing a fixture, update its archive URL, package version, member path,
raw and fixture hashes, applicable shared license files, and native topology
assertions in the same commit.

## `mant` parsing verification

On 2026-07-21, a batch scan exercised **3,745 topic/section requests** from
43 Arch Linux packages through ManT's bundled libmandoc path.

No parser crash was observed. This count measures successful completion, not
perfect structural or rendering fidelity for every page; see the parent
README for the corpus limitations.

[VERIFIED_TOPICS.txt](VERIFIED_TOPICS.txt) records the exact scope and
representative topics, grouped by source package.

| Package group | Topics | Notes |
| ------------- | ------ | ----- |
| tcl/tk | 1,199 | Tcl commands and C APIs (section n) |
| library (s3) | 464 | ncurses, util-linux, and other library functions |
| coreutils | 118 | Complete GNU coreutils set (ls, cat, cp, ...) |
| util-linux | 102 | mount, fdisk, losetup, ... |
| curl | 93 | libcurl APIs (section 3) |
| graphviz | 46 | Graph layout tools and C APIs |
| procps-ng | 31 | ps, top, kill, free, ... |
| mtools | 30 | FAT filesystem tools |
| openssh | 14 | ssh, sshd, scp, sftp, ... |
| mandoc | 12 | mandoc toolchain |
| system (s8) | 11 | System administration tools |
| Other (bash, cpio, diffutils, findutils, gnuplot, grep, mutt, nmap, parted, recode, rsync, screen, sed, socat, tmux, xterm) | 1–5 each | — |

[GNU coreutils]: https://www.gnu.org/software/coreutils/
[Git]: https://git-scm.com/
[GCC]: https://gcc.gnu.org/
[LLVM Clang]: https://clang.llvm.org/
[GNU tar]: https://www.gnu.org/software/tar/
[GNU gawk]: https://www.gnu.org/software/gawk/
[Rsync]: https://rsync.samba.org/
[POSIX sh]: https://pubs.opengroup.org/onlinepubs/9699919799/utilities/sh.html
[libarchive]: https://libarchive.org/
[libbsd]: https://libbsd.freedesktop.org/
[Arch `coreutils` 9.11-2]: https://archive.archlinux.org/packages/c/coreutils/coreutils-9.11-2-x86_64.pkg.tar.zst
[Arch `git` 2.55.0-1]: https://archive.archlinux.org/packages/g/git/git-2.55.0-1-x86_64.pkg.tar.zst
[Arch `gcc` 16.1.1+r346+g4e03491b401d-4]: https://archive.archlinux.org/packages/g/gcc/gcc-16.1.1%2Br346%2Bg4e03491b401d-4-x86_64.pkg.tar.zst
[Arch `clang` 22.1.8-1]: https://archive.archlinux.org/packages/c/clang/clang-22.1.8-1-x86_64.pkg.tar.zst
[Arch `tar` 1.35-2]: https://archive.archlinux.org/packages/t/tar/tar-1.35-2-x86_64.pkg.tar.zst
[Arch `gawk` 5.2.0-1]: https://archive.archlinux.org/packages/g/gawk/gawk-5.2.0-1-x86_64.pkg.tar.zst
[Arch `rsync` 3.4.3-1]: https://archive.archlinux.org/packages/r/rsync/rsync-3.4.3-1-x86_64.pkg.tar.zst
[Arch `man-pages` 6.18-1]: https://archive.archlinux.org/packages/m/man-pages/man-pages-6.18-1-any.pkg.tar.zst
[Arch `libarchive` 3.8.9-1]: https://archive.archlinux.org/packages/l/libarchive/libarchive-3.8.9-1-x86_64.pkg.tar.zst
[Arch `libbsd` 0.12.2-2]: https://archive.archlinux.org/packages/l/libbsd/libbsd-0.12.2-2-x86_64.pkg.tar.zst
[GPL-2.0-only]: ../LICENSES/GPL-2.0-only.txt
[GPL-3.0-or-later]: ../LICENSES/GPL-3.0-or-later.txt
[GFDL-1.3-invariants-or-later]: ../LICENSES/GFDL-1.3-invariants-or-later.txt
[Apache-2.0 WITH LLVM-exception]: ../LICENSES/LLVM.txt
[gawk manual-page permission]: ../LICENSES/GAWK-MANPAGE.txt
[POSIX manual notice]: ../LICENSES/POSIX-COPYRIGHT.txt
[BSD-2-Clause]: archive_entry_stat.3
