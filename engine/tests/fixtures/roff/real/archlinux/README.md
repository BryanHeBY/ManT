# Arch Linux fixtures

These fixtures contain roff manual bytes extracted from the immutable Arch
Linux Archive packages listed below. The original `*.1.gz` fixtures were
recorded in ManT on 2026-07-19. The `gawk` and `rsync` package references were
verified on 2026-07-23 after those fixtures were added: their gzip members were
decompressed without changing the roff bytes and recompressed as zstd to retain
coverage of ManT's in-process zstd decoder.

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

The two recompressed fixtures preserve these exact decompressed roff hashes:

- `gawk.1`: `d28fc0d5bfdc08f85faaa6267b14223520967f9fdf0730550f12fee880b2ca31`
- `rsync.1`: `12417d699e494cd5154195df53762f0043e2ffe3997634c4e5f4afc209f87d45`

The corresponding immutable package archives have these SHA-256 values:

- `gawk-5.2.0-1-x86_64.pkg.tar.zst`: `dd6a14cb65eec0754eb0d77a373bc685cff2776133007251e35593a3de8045f6`
- `rsync-3.4.3-1-x86_64.pkg.tar.zst`: `f2ad0dcc4d7022cb7f04c4da716be067b93a95fc246f2c0259cb2dbb880684e5`

The GCC manual embeds its own GFDL invariant sections, front-cover text, and
back-cover text. Those page-specific notices remain in `gcc.1.gz`; the shared
[`GFDL` text](../LICENSES/GFDL-1.3-invariants-or-later.txt) supplies the
complete license it references. [`LLVM.txt`](../LICENSES/LLVM.txt) is the full
license text shipped with the matching Arch Clang package, including the Apache
License 2.0, LLVM exception, and legacy LLVM notice. The gawk page's own
copying permission is retained in its `COPYING PERMISSIONS` section and
transcribed in [`GAWK-MANPAGE.txt`](../LICENSES/GAWK-MANPAGE.txt).

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

## `mant` 解析验证

2026-07-21 对从 Arch Linux 下载的 43 个软件包中的 **3,745 个 topic/section
请求**执行了 `mant --force-libmandoc` 批量扫描。

观察结果：未出现解析崩溃。该统计衡量进程完成性，不代表每页的结构或排版
均完全保真；已知限制见父目录 README。

扫描范围与代表性 topic 见 [VERIFIED_TOPICS.txt](VERIFIED_TOPICS.txt)，按来源软件包分组。

| 软件包 | Topics | 备注 |
| ------ | ------ | ---- |
| tcl/tk | 1,199 | Tcl 命令和 C API（section n） |
| library (s3) | 464 | ncurses、util-linux 等库函数 |
| coreutils | 118 | GNU coreutils 全量（ls, cat, cp, ...） |
| util-linux | 102 | mount, fdisk, losetup, ... |
| curl | 93 | libcurl API（section 3） |
| graphviz | 46 | 图形布局工具和 C API |
| procps-ng | 31 | ps, top, kill, free, ... |
| mtools | 30 | FAT 文件系统工具 |
| openssh | 14 | ssh, sshd, scp, sftp, ... |
| mandoc | 12 | mandoc 工具链 |
| system (s8) | 11 | 系统管理工具 |
| 其他（bash, cpio, diffutils, findutils, gnuplot, grep, mutt, nmap, parted, recode, rsync, screen, sed, socat, tmux, xterm） | 1–5 各 | — |

[GNU coreutils]: https://www.gnu.org/software/coreutils/
[Git]: https://git-scm.com/
[GCC]: https://gcc.gnu.org/
[LLVM Clang]: https://clang.llvm.org/
[GNU tar]: https://www.gnu.org/software/tar/
[GNU gawk]: https://www.gnu.org/software/gawk/
[Rsync]: https://rsync.samba.org/
[Arch `coreutils` 9.11-2]: https://archive.archlinux.org/packages/c/coreutils/coreutils-9.11-2-x86_64.pkg.tar.zst
[Arch `git` 2.55.0-1]: https://archive.archlinux.org/packages/g/git/git-2.55.0-1-x86_64.pkg.tar.zst
[Arch `gcc` 16.1.1+r346+g4e03491b401d-4]: https://archive.archlinux.org/packages/g/gcc/gcc-16.1.1%2Br346%2Bg4e03491b401d-4-x86_64.pkg.tar.zst
[Arch `clang` 22.1.8-1]: https://archive.archlinux.org/packages/c/clang/clang-22.1.8-1-x86_64.pkg.tar.zst
[Arch `tar` 1.35-2]: https://archive.archlinux.org/packages/t/tar/tar-1.35-2-x86_64.pkg.tar.zst
[Arch `gawk` 5.2.0-1]: https://archive.archlinux.org/packages/g/gawk/gawk-5.2.0-1-x86_64.pkg.tar.zst
[Arch `rsync` 3.4.3-1]: https://archive.archlinux.org/packages/r/rsync/rsync-3.4.3-1-x86_64.pkg.tar.zst
[GPL-2.0-only]: ../LICENSES/GPL-2.0-only.txt
[GPL-3.0-or-later]: ../LICENSES/GPL-3.0-or-later.txt
[GFDL-1.3-invariants-or-later]: ../LICENSES/GFDL-1.3-invariants-or-later.txt
[Apache-2.0 WITH LLVM-exception]: ../LICENSES/LLVM.txt
[gawk manual-page permission]: ../LICENSES/GAWK-MANPAGE.txt
