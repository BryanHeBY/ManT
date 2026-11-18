# BSD symbol declarations

These complete gzip members match both the retained release archives and
the pages in [mant-roff-corpus 832e18b](https://github.com/BryanHeBY/mant-roff-corpus/tree/832e18b3645884dd1d04dc34ebd656015e5a8e71).
The fixture basenames carry a distribution prefix. The stored bytes are
otherwise identical to the upstream release members.

| Fixture | Release member | Evidence |
| --- | --- | --- |
| `freebsd-errno.2.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man2/errno.2.gz` | The 95 numbered error names bind on their original HEAD lines. |
| `freebsd-ioctl.2.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man2/ioctl.2.gz` | `FIONREAD` keeps its range and body while `int` supplies type text. |
| `freebsd-ppi.4.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man4/ppi.4.gz` | The two comma-delimited constants resolve to one description. |
| `dragonflybsd-sc.4.gz` | DragonFly BSD 6.4.2 x86-64 release ISO, `usr/share/man/man4/sc.4.gz` | The `GIO_COLOR` binding survives its pointer argument. |

Each member retains its complete BSD redistribution notice. The
[manifest](manifest.json) records the licenses, artifact and member hashes,
and results for all five pinned CVS profiles. Native profile output hashes
use the fixture basename from this directory under `LC_ALL=C` and `TZ=UTC`.
