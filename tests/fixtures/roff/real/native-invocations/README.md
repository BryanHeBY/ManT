# Native invocation sources

These four complete manuals were retrieved from the retained audit corpus on
2026-11-19. The FreeBSD files are original gzip members of the amd64
15.1-RELEASE `base.txz` archive. The illumos file is the exact `zfs(8)` source
at commit `e8f5c080cc0b7997410d860afd787df30ba1cf2d`. The fixture files
retain their upstream notices and are covered by [CDDL 1.0](../LICENSES/CDDL-1.0.txt).

| Manual | Regression distinction |
| --- | --- |
| `dtrace(1)` | The declaration binds `evaltime` and keeps `exec\|preinit\|postinit\|main` in its complete form. |
| `zfs-allow(8)` | A command with several alternative operands keeps its invocation name and form. |
| `zfs-mount(8)` | The `-R` token inside an invocation remains distinct from the separate `-R` option definition. |
| illumos `zfs(8)` | The standalone `volume` definition remains distinct from the operand in `zfs allow` forms. |

[The manifest](manifest.json) records the exact origin, member and decoded
hashes, byte counts, and fixed CVS ASCII, UTF-8, HTML, tree and lint receipts.

[FreeBSD release archive](https://download.freebsd.org/releases/amd64/amd64/15.1-RELEASE/base.txz)
[illumos source](https://github.com/illumos/illumos-gate/blob/e8f5c080cc0b7997410d860afd787df30ba1cf2d/usr/src/man/man8/zfs.8)
