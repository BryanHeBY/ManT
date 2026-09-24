# Arch Linux fixtures

These fixtures contain roff manual bytes extracted from the immutable Arch
Linux Archive packages listed below, plus one exact manual from the pinned
upstream yay source release. The original `*.1.gz` fixtures were
recorded in ManT on 2026-07-19. The `gawk` and `rsync` package references were
verified on 2026-07-23 after those fixtures were added: their gzip members were
decompressed without changing the roff bytes and recompressed as zstd to retain
coverage of ManT's in-process zstd decoder.

`archive_entry_stat.3` was added on 2026-08-19 from Arch's libarchive package.
It stores the exact decompressed roff bytes so the declaration-unit regression
remains directly inspectable; its source hash matches the host-audit ledger.

`bsdunzip.1` was added on 2026-08-29 from the same Arch libarchive package
after a lowering review exposed two distinct option heads sharing the body of
the following mdoc list item. It also stores the exact decompressed roff bytes.

`expand_number.3bsd` was added on 2026-08-23 from Arch's libbsd package after
a mandoc comparison exposed a missing comma between multiple operands of one
mdoc `Fa` invocation. It likewise stores the exact decompressed source and
retains the complete page-specific BSD-2-Clause notice.

`zip_source_function.3` was added the same day from Arch's libzip package
after the aligned mandoc audit exposed the same operand-loss class inside an
out-of-SYNOPSIS `Fo` function-pointer declaration. It retains the complete
page-specific BSD-3-Clause notice.

`libpipeline.3.gz` was added on 2026-09-03 from Arch's libpipeline package
after a target-conservation audit showed that function identities moved onto
paragraph nodes by libmandoc disappeared during structural lowering. The
original compressed package member is retained unchanged.

On 2026-09-24, the R04 semantic-query corpus gained the original packaged
gzip members for tmux, zsh, gzip, zip and btrfs-subvolume. Their package
archives were retained in the local Arch package cache and checked by SHA-256
before extraction. The gzip 1.14-2
archive is intentionally older than the installed 1.15-1 package: its manual
bytes match the previously reviewed `ENTRY_QUERY_GOLD.json` source identity.
The yay fixture is the exact `doc/yay.8` member of the upstream v13.0.1 source
tarball; that member is byte-identical to the locally built package's manual
after decompression. It is stored uncompressed and does not depend on the
locally built package for reproduction.
These additions do not retroactively change the 2026-07-21 parser-scan counts
in `VERIFIED_TOPICS.txt`; R04 query evidence is tracked separately.

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
| `bsdunzip.1` | [libarchive], [Arch `libarchive` 3.8.9-1] | `usr/share/man/man1/bsdunzip.1.gz` | Exact decompressed source | [libarchive BSD-2-Clause] | `a271c9471543684df90d1a3f0d988628588ed8ec4f49bb2f7e402fc752ff4a83` |
| `expand_number.3bsd` | [libbsd], [Arch `libbsd` 0.12.2-2] | `usr/share/man/man3/expand_number.3bsd.gz` | Exact decompressed source | [BSD-2-Clause] | `4e0d2bd2af63de49f6c55ce96ae07b52a6c2214d837f4c33eec47699ca19de03` |
| `zip_source_function.3` | [libzip], [Arch `libzip` 1.11.4-1] | `usr/share/man/man3/zip_source_function.3.gz` | Exact decompressed source | [BSD-3-Clause] | `73a409d297a001c885fc00092a92105f30fe4e6ecca0b57d06c6fecc40b5b898` |
| `libpipeline.3.gz` | [libpipeline], [Arch `libpipeline` 1.5.8-1] | `usr/share/man/man3/libpipeline.3.gz` | Original member | [GPL-3.0-or-later] | `20d992e497e6f86a3abfc218aa349620a05e7b58250db0beefafedc07c7aed7e` |
| `tmux.1.gz` | [tmux], [Arch `tmux` 3.7_c-1] | `usr/share/man/man1/tmux.1.gz` | Original member | [tmux license] | `fe9b35d424d72b5c6155d3bc6b351724e034d4034746b7e008523a47b71cba7d` |
| `zsh.1.gz` | [Z shell], [Arch `zsh` 5.9.2-1] | `usr/share/man/man1/zsh.1.gz` | Original member | [Zsh license] | `48e46d49ac1d6678e818cbbf5e099d6c7406589a79e9757d3dbe2c2eaa2e707a` |
| `gzip.1.gz` | [GNU gzip], [Arch `gzip` 1.14-2] | `usr/share/man/man1/gzip.1.gz` | Original member | [gzip manual permission] | `91ec1c846bb4deeb3339f718988737a4957de335b3860923da6bfd302eada4c0` |
| `zip.1.gz` | [Info-ZIP], [Arch `zip` 3.0-14] | `usr/share/man/man1/zip.1.gz` | Original member | [Info-ZIP license] | `9f984b4b87db8f5ccf78dd28c9907a601f8c0a431d4efd64195b5006cf189818` |
| `btrfs-subvolume.8.gz` | [Btrfs progs], [Arch `btrfs-progs` 7.1-1] | `usr/share/man/man8/btrfs-subvolume.8.gz` | Original member | [GPL-2.0-only] | `c00ade46fa983503b4fe2f0457f719142d00c2e4a09a9d69054538a2032d7a78` |
| `yay.8` | [yay], [upstream yay v13.0.1 source] | `yay-13.0.1/doc/yay.8` | Exact upstream source | [GPL-3.0-or-later] | `7f5115d15c9647b77bbc4aa838dff90818a20f03763f1557e1a0e36292ab614e` |

The recompressed fixtures and the newly frozen gzip members preserve these
exact decompressed roff hashes:

- `gawk.1`: `d28fc0d5bfdc08f85faaa6267b14223520967f9fdf0730550f12fee880b2ca31`
- `rsync.1`: `12417d699e494cd5154195df53762f0043e2ffe3997634c4e5f4afc209f87d45`
- `sh.1p`: `8caa52a52fcb46e6e4e38105408dac290b66865cf712f6897d81bfa438f16b2d`
- `tmux.1`: `cbedf24cf75128a6794a24a4c380307f73515047945b6736933d313af4623ae7`
- `zsh.1`: `fee817f32be2ca893147affc5b9f82dce17ad8f44d77fcb40839450ea94dd15d`
- `gzip.1`: `107c35463c2fb970318b34ccca09db100e1b99bfab793196eab68ef91d350b78`
- `zip.1`: `b8cd4f0980a6a3abd243c00a146bf336564b2bf6c270d1101f541da4d9796623`
- `btrfs-subvolume.8`: `7aad0096c8fa69c22b8487f45ace7e38f820d4b7dc1a5877b2e95ebd9a28ae96`

The uncompressed `yay.8` fixture and its upstream source member have the same
SHA-256: `7f5115d15c9647b77bbc4aa838dff90818a20f03763f1557e1a0e36292ab614e`.

The corresponding package and source archives have these SHA-256 values:

- `gawk-5.2.0-1-x86_64.pkg.tar.zst`: `dd6a14cb65eec0754eb0d77a373bc685cff2776133007251e35593a3de8045f6`
- `rsync-3.4.3-1-x86_64.pkg.tar.zst`: `f2ad0dcc4d7022cb7f04c4da716be067b93a95fc246f2c0259cb2dbb880684e5`
- `man-pages-6.18-1-any.pkg.tar.zst`: `f03bbc27c6c14aed6c009a4780e618cf57c0eb9cdca390a3c4eacc600d197ba3`
- `libarchive-3.8.9-1-x86_64.pkg.tar.zst`: `07b7aaec008cc4892cb2da0c599a16438d2e70869a01b90c4e20bf153f58c3b3`
- `libbsd-0.12.2-2-x86_64.pkg.tar.zst`: `e26194849786b0202828a348be3f4b90d410604cd7d48f113a4301584a49895a`
- `libzip-1.11.4-1-x86_64.pkg.tar.zst`: `e6bc733cdc738d317d94f1906eb86229be174aa8362ad7dbb03ae68d9eb3dbf6`
- `libpipeline-1.5.8-1-x86_64.pkg.tar.zst`: `35caa28ccb5f00d06f043a68004c406d4985dd4557669e972c7fafeb1af3be5f`
- `tmux-3.7_c-1-x86_64.pkg.tar.zst`: `dacfb3eb339bb87bd08be4b8134543ae4911243f8c90d0b9ef8dd98968cf40df`
- `zsh-5.9.2-1-x86_64.pkg.tar.zst`: `92982239da698cd541ed2f8c366b6d152c06d0aad22c78f7313f364edab15433`
- `gzip-1.14-2-x86_64.pkg.tar.zst`: `e08b33c6aa9a19108d7941a78a6d137e3f700a544d21f14bf863d3c392c3a88e`
- `zip-3.0-14-x86_64.pkg.tar.zst`: `b1b09aedec942330aae0715be52066455aac418f8f8c39ee3db00d7b3d92cd0b`
- `btrfs-progs-7.1-1-x86_64.pkg.tar.zst`: `5151b1c783a376bc85c04bb7ddc6f816668db60d9e864a88042dbb286b146456`
- `yay-13.0.1.tar.gz`: `b77454bce87110180a1b6664c2d260de78124c9894b71101610ba84f551eb0d0` (upstream source, also verified against the AUR `PKGBUILD` at commit `cb43f84828ab4f9700f7c6f9c6d7a923d4cfaff0`)

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
The libarchive `archive_entry_stat` and libbsd pages retain their complete
BSD-2-Clause notices at the start of each fixture. The SPDX-identified
`bsdunzip` page's complete page-specific terms are reproduced in
[`LIBARCHIVE-BSD-2-Clause.txt`](../LICENSES/LIBARCHIVE-BSD-2-Clause.txt). The
libzip page likewise retains its complete BSD-3-Clause notice.
The new tmux, zsh and zip pages use the exact license files shipped in their
matching Arch packages: [`TMUX-LICENSE.txt`](../LICENSES/TMUX-LICENSE.txt),
[`ZSH-LICENSE.txt`](../LICENSES/ZSH-LICENSE.txt) and
[`INFO-ZIP-LICENSE.txt`](../LICENSES/INFO-ZIP-LICENSE.txt). The gzip manual's
own complete copying permission remains embedded in its `COPYRIGHT NOTICE`
section and is transcribed in
[`GZIP-MANPAGE.txt`](../LICENSES/GZIP-MANPAGE.txt); it is distinct from the
package's program license. The Btrfs package declares GPL-2.0-only, whose
complete text is already shared above. The yay
source tarball contains the full GPL-3.0 text in `LICENSE`, covered by the
shared [GPL-3.0-or-later] text.

## Reproducing a fixture

Download the exact archive package and extract the existing compressed member.
For the original gzip fixtures, do not recompress it. For example:

```sh
curl -LO https://archive.archlinux.org/packages/c/coreutils/coreutils-9.11-2-x86_64.pkg.tar.zst
bsdtar -xOf coreutils-9.11-2-x86_64.pkg.tar.zst \
  usr/share/man/man1/ls.1.gz > ls.1.gz
sha256sum ls.1.gz
```

For the upstream yay source, verify the tarball SHA above and extract its
uncompressed manual member directly:

```sh
curl -L -o yay-13.0.1.tar.gz https://github.com/Jguer/yay/archive/v13.0.1.tar.gz
sha256sum yay-13.0.1.tar.gz
bsdtar -xOf yay-13.0.1.tar.gz yay-13.0.1/doc/yay.8 > yay.8
sha256sum yay.8
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

When replacing a fixture, update its archive URL, package or release version,
member path, raw and fixture hashes, applicable shared license files, and
native topology assertions in the same commit.

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
[libzip]: https://libzip.org/
[libpipeline]: https://nongnu.org/libpipeline/
[tmux]: https://github.com/tmux/tmux
[Z shell]: https://www.zsh.org/
[GNU gzip]: https://www.gnu.org/software/gzip/
[Info-ZIP]: https://infozip.sourceforge.net/
[Btrfs progs]: https://btrfs.readthedocs.io/
[yay]: https://github.com/Jguer/yay
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
[Arch `libzip` 1.11.4-1]: https://archive.archlinux.org/packages/l/libzip/libzip-1.11.4-1-x86_64.pkg.tar.zst
[Arch `libpipeline` 1.5.8-1]: https://archive.archlinux.org/packages/l/libpipeline/libpipeline-1.5.8-1-x86_64.pkg.tar.zst
[Arch `tmux` 3.7_c-1]: https://archive.archlinux.org/packages/t/tmux/tmux-3.7_c-1-x86_64.pkg.tar.zst
[Arch `zsh` 5.9.2-1]: https://archive.archlinux.org/packages/z/zsh/zsh-5.9.2-1-x86_64.pkg.tar.zst
[Arch `gzip` 1.14-2]: https://archive.archlinux.org/packages/g/gzip/gzip-1.14-2-x86_64.pkg.tar.zst
[Arch `zip` 3.0-14]: https://archive.archlinux.org/packages/z/zip/zip-3.0-14-x86_64.pkg.tar.zst
[Arch `btrfs-progs` 7.1-1]: https://archive.archlinux.org/packages/b/btrfs-progs/btrfs-progs-7.1-1-x86_64.pkg.tar.zst
[upstream yay v13.0.1 source]: https://github.com/Jguer/yay/archive/v13.0.1.tar.gz
[GPL-2.0-only]: ../LICENSES/GPL-2.0-only.txt
[GPL-3.0-or-later]: ../LICENSES/GPL-3.0-or-later.txt
[GFDL-1.3-invariants-or-later]: ../LICENSES/GFDL-1.3-invariants-or-later.txt
[Apache-2.0 WITH LLVM-exception]: ../LICENSES/LLVM.txt
[gawk manual-page permission]: ../LICENSES/GAWK-MANPAGE.txt
[POSIX manual notice]: ../LICENSES/POSIX-COPYRIGHT.txt
[BSD-2-Clause]: archive_entry_stat.3
[libarchive BSD-2-Clause]: ../LICENSES/LIBARCHIVE-BSD-2-Clause.txt
[BSD-3-Clause]: zip_source_function.3
[tmux license]: ../LICENSES/TMUX-LICENSE.txt
[Zsh license]: ../LICENSES/ZSH-LICENSE.txt
[gzip manual permission]: ../LICENSES/GZIP-MANPAGE.txt
[Info-ZIP license]: ../LICENSES/INFO-ZIP-LICENSE.txt
