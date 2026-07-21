# Debian sid gzip fixtures

These fixtures are the original compressed roff manual members extracted from
the Debian sid (unstable) binary packages listed below. The package references
and fixture bytes were recorded in ManT on 2026-07-21. No decompression or
recompression occurs: each committed `*.gz` file is the package member named
in its row, so its fixture SHA-256 is also the compressed-member hash.

They extend the real-man corpus to Debian's generator output and exercise
section-7 macro reference pages alongside the existing section-1 corpus. The
groff pages expose man(7) macro-package internals, while mt-gnu exercises
cpio's tape-archiver manual with embedded roff escapes.

| Fixture | Upstream and Debian package | Package member | Fixture license | Fixture SHA-256 |
| --- | --- | --- | --- | --- |
| `mt-gnu.1.gz` | [GNU cpio], [Debian `cpio` 2.15+dfsg-2.1] | `usr/share/man/man1/mt-gnu.1.gz` | [GPL-3.0-or-later] | `190561ec27d5b16e5c9ef634e8caac722d60c8635843994297942907c3ff9ba0` |
| `groff_me.7.gz` | [GNU groff], [Debian `groff` 1.24.1-1] | `usr/share/man/man7/groff_me.7.gz` | BSD-3-Clause (UCB) + [GPL-3.0-or-later] | `170a171a65fd9c3082453b85425f71749ce00535ca7d0dbca7a4668c2799d554` |
| `groff_man_style.7.gz` | [GNU groff], [Debian `groff` 1.24.1-1] | `usr/share/man/man7/groff_man_style.7.gz` | [GFDL-1.3-invariants-or-later] | `836bb1bf1827fc4d057abb50f254a428b62f3857dc969a200a8fa1a6113a4b2b` |

groff_me.7 retains the Regents of the University of California BSD-3-Clause
copyright (with the deleted clause 3 marked inline) and the Free Software
Foundation's GPL-3.0-or-later for FSF modifications; the BSD terms are
reproduced in the embedded file header. groff_man_style.7 is licensed under
the GFDL without Invariant Sections; the shared
[GFDL text](../LICENSES/GFDL-1.3-invariants-or-later.txt) supplies the
referenced license. The mt-gnu page from cpio carries the standard
GPL-3.0-or-later.

## Reproducing a fixture

Download the exact Debian package and extract the compressed member without
recompressing it. For example:

```sh
curl -LO https://deb.debian.org/debian/pool/main/g/groff/groff_1.24.1-1_amd64.deb
dpkg-deb --fsys-tarfile groff_1.24.1-1_amd64.deb \
  | tar -xOf - ./usr/share/man/man7/groff_me.7.gz > groff_me.7.gz
sha256sum groff_me.7.gz
```

When replacing a fixture, update its package pool URL, version, member path,
hash, and applicable license references in the same commit.

## `mant` 解析验证

2026-07-21 对从 Debian sid 下载的 29 个软件包中的 **491 个 topic/section
请求**执行了 `mant --force-libmandoc` 批量扫描。

观察结果：未出现解析崩溃。该统计衡量进程完成性，不代表每页的结构或排版
均完全保真；已知限制见父目录 README。

完整 topic 清单见 [VERIFIED_TOPICS.txt](VERIFIED_TOPICS.txt)，按来源软件包分组。

| 软件包 | Topics | 备注 |
| ------ | ------ | ---- |
| coreutils | 100 | GNU coreutils 全量 |
| util-linux | 49 | mount, fdisk, ... |
| groff | 41 | groff 工具链和宏包 |
| mtools | 30 | FAT 文件系统工具 |
| mandoc | 12 | mandoc 工具链 |
| procps-ng | 6 | ps, top, ... |
| 其他 | 各1–6 | bash, cpio, curl, diffutils, findutils, gawk, graphviz, grep, mutt, nmap, parted, recode, rsync, screen, sed, socat, tmux, units, xterm |

[GNU cpio]: https://www.gnu.org/software/cpio/
[GNU groff]: https://www.gnu.org/software/groff/
[Debian `cpio` 2.15+dfsg-2.1]: https://packages.debian.org/sid/cpio
[Debian `groff` 1.24.1-1]: https://packages.debian.org/sid/groff
[GPL-3.0-or-later]: ../LICENSES/GPL-3.0-or-later.txt
[GFDL-1.3-invariants-or-later]: ../LICENSES/GFDL-1.3-invariants-or-later.txt
