# Definition and table display fixtures

These complete manuals were copied without changes from [the retained corpus](https://github.com/BryanHeBY/mant-roff-corpus/tree/832e18b3645884dd1d04dc34ebd656015e5a8e71/corpora). Each entry in [manifest.json](manifest.json) records the original source member, the matching source and fixture hashes, the original notice, and receipts for five pristine CVS profiles.

| Fixture | Origin | Coverage |
| --- | --- | --- |
| `illumos-ctfdiff.1` | [illumos-gate `ctfdiff.1`](https://github.com/illumos/illumos-gate/blob/e8f5c080cc0b7997410d860afd787df30ba1cf2d/usr/src/man/man1/ctfdiff.1), copied November 18, 2026 | Compact `Bd` places the descriptions of exit codes 0 and 1 below the label without an additional blank line. The `.D1` descriptions of 2 and 3 provide a control. |
| `illumos-fdio.4i` | [illumos-gate `fdio.4i`](https://github.com/illumos/illumos-gate/blob/e8f5c080cc0b7997410d860afd787df30ba1cf2d/usr/src/man/man4i/fdio.4i), copied November 19, 2026 | Four offset displays after `.El` remain section content. Explain and the selected ioctl nodes exclude them. |
| `freebsd-_umtx_op.2.gz` | `./usr/share/man/man2/_umtx_op.2.gz` in [FreeBSD 15.1-RELEASE amd64 `base.txz`](https://download.freebsd.org/releases/amd64/amd64/15.1-RELEASE/base.txz), copied November 19, 2026 | Five synchronization structure declarations stay outside the preceding `USYNC_PROCESS_SHARED` item. |
| `freebsd-rc.subr.8.gz` | `./usr/share/man/man8/rc.subr.8.gz` in the same FreeBSD archive, copied November 19, 2026 | Repeated displays within a `.Bl -column` list each require evidence from their own cell. |

The illumos files retain their original notices under the [CDDL](../LICENSES/CDDL-1.0.txt). The FreeBSD release members retain their [FreeBSD Foundation](../LICENSES/FREEBSD-UMTX-OP-BSD-2-Clause.txt) and [NetBSD Foundation](../LICENSES/NETBSD-RC-SUBR-BSD-2-Clause.txt) BSD-2-Clause notices.
