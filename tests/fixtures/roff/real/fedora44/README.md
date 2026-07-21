# Fedora Linux 44 zstd fixtures

These fixtures are lossless zstd recompressions of the original roff manual
members shipped by the official Fedora Linux 44 RPMs listed below. The binary
RPMs and matching source RPMs were acquired from the Fedora 44 `Everything`
repositories on 2026-07-20. This makes the fixture's distribution package,
source package, exact package member, and applicable upstream license
independently verifiable.

Fedora packages these pages as `*.1.gz`. To exercise ManT's in-process zstd
decoder, each gzip member was decompressed without changing its roff bytes and
then compressed with Zstandard CLI 1.5.7 at level 19. The `Raw roff SHA-256`
column identifies those unchanged input bytes; `Fixture SHA-256` identifies the
committed zstd stream. In particular, `tar.1.zst` decompresses to the same
bytes as the parent Arch fixture `tar.1.gz`.

| Fixture | Fedora 44 RPM / source RPM | Original member | Fixture license | Raw roff SHA-256 | Fixture SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `clang.1.zst` | [clang 22.1.1-2.fc44] / [llvm source RPM] | `usr/share/man/man1/clang.1.gz` | [Apache-2.0 WITH LLVM-exception] | `8f727b7a3966a90989f474259bab124fdb3935913dffcb6912c1237a63b2b241` | `6f2673fa1cee4e1c7a81638645458cd16e1af5af05644e174d026d87c6c38228` |
| `gcc.1.zst` | [gcc 16.0.1-0.10.fc44] / [gcc source RPM] | `usr/share/man/man1/gcc.1.gz` | [GFDL-1.3-invariants-or-later] | `531ca21b660c1fbd294218e921d834adfaccd61c6f400a2a5d844759f40fc034` | `7a1ec76f50702b05c389ba777c5dee70155c05781f05056a9dafb07154625699` |
| `git.1.zst` | [git-core-doc 2.53.0-1.fc44] / [git source RPM] | `usr/share/man/man1/git.1.gz` | [GPL-2.0-only] | `2ee1c5dd84a69dfc91d84840e943e7df0ee06b86bf27eb4bb9369c415f3351c1` | `43e55d11719d6b4db16e1da7228e0446a64f4cef4a0fb449ea6c84f8e37f2e03` |
| `tar.1.zst` | [tar 1.35-8.fc44] / [tar source RPM] | `usr/share/man/man1/tar.1.gz` | [GPL-3.0-or-later] | `3a85ebdd1601114e7c8c1dfa35726f8394d59412887242ffc6ef49ec021017ee` | `97ba77e351ef5561f748b12401f344cfca374998e080dc7db1f8914e0c09e63e` |

The original binary RPM SHA-256 values are recorded here as an additional
provenance check:

- `clang-22.1.1-2.fc44.x86_64.rpm`: `16dd1ab16f194af40c83182025faa29c3310842bc8d2b127ddc8151977f08cc8`
- `gcc-16.0.1-0.10.fc44.x86_64.rpm`: `5410123363bb8a6e3ac5d00448619957f8062e4769a434ba28cf61e9f2524871`
- `git-core-doc-2.53.0-1.fc44.noarch.rpm`: `12e2db464bfa10726554860c7a63df9216a23d2553cc93126731886de80b114b`
- `tar-1.35-8.fc44.x86_64.rpm`: `5b9cc358930c4fddec59c3cc13d8ebc9fbb315019a9c96dc24adaa2e38d07568`

The complete license texts are shared with the corresponding fixtures in the
parent `LICENSES/` directory. The GCC manual's invariant sections and cover
texts remain embedded verbatim in `gcc.1.zst`; `LLVM.txt` is byte-identical to
the license file shipped in the Fedora Clang RPM. These third-party licenses
govern only the fixtures, not ManT's repository-level Apache-2.0 license.

## Reproducing a fixture

Download the pinned binary RPM, extract its compressed man-page member, and
recompress only the unchanged roff bytes:

```sh
curl -LO https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/x86_64/os/Packages/t/tar-1.35-8.fc44.x86_64.rpm
bsdtar -xOf tar-1.35-8.fc44.x86_64.rpm ./usr/share/man/man1/tar.1.gz \
  | gzip -dc > tar.1
zstd -19 -f -o tar.1.zst tar.1
sha256sum tar.1 tar.1.zst
```

When replacing a fixture, update both RPM URLs, source RPM URL, package and
raw-roff hashes, fixture hash, applicable license files, and topology
assertions in the same commit.

## `mant-cli` 解析验证

2026-07-21 对从 Fedora Linux 44 下载的 20 个软件包中的 **246 页**
（232 个 distinct topic）执行了 `mant-cli --force-libmandoc` 批量扫描。

结果：**100% 成功**，0 崩溃，0 解析失败。

完整 topic 清单见 [VERIFIED_TOPICS.txt](VERIFIED_TOPICS.txt)，按来源软件包分组。

| 软件包 | Topics | 备注 |
| ------ | ------ | ---- |
| tcl/tk | 104 | Tcl 命令和 C API |
| groff | 36 | groff 工具链和宏包 |
| mtools | 27 | FAT 文件系统工具 |
| mandoc | 6 | mandoc 工具链 |
| mutt | 5 | 邮件客户端 |
| nmap | 2 | 网络扫描 |
| rsync / socat | 各3 | — |
| gawk | 2 | GNU awk |
| 其他（cpio, gnuplot, graphviz, recode, screen, tmux, units, xterm） | 各1–3 | — |

[clang 22.1.1-2.fc44]: https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/x86_64/os/Packages/c/clang-22.1.1-2.fc44.x86_64.rpm
[llvm source RPM]: https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/source/tree/Packages/l/llvm-22.1.1-2.fc44.src.rpm
[gcc 16.0.1-0.10.fc44]: https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/x86_64/os/Packages/g/gcc-16.0.1-0.10.fc44.x86_64.rpm
[gcc source RPM]: https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/source/tree/Packages/g/gcc-16.0.1-0.10.fc44.src.rpm
[git-core-doc 2.53.0-1.fc44]: https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/x86_64/os/Packages/g/git-core-doc-2.53.0-1.fc44.noarch.rpm
[git source RPM]: https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/source/tree/Packages/g/git-2.53.0-1.fc44.src.rpm
[tar 1.35-8.fc44]: https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/x86_64/os/Packages/t/tar-1.35-8.fc44.x86_64.rpm
[tar source RPM]: https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Everything/source/tree/Packages/t/tar-1.35-8.fc44.src.rpm
[Apache-2.0 WITH LLVM-exception]: ../LICENSES/LLVM.txt
[GFDL-1.3-invariants-or-later]: ../LICENSES/GFDL-1.3-invariants-or-later.txt
[GPL-2.0-only]: ../LICENSES/GPL-2.0-only.txt
[GPL-3.0-or-later]: ../LICENSES/GPL-3.0-or-later.txt
