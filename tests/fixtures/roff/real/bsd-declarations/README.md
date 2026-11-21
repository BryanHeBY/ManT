# BSD symbol declarations

These complete manual members match both the retained release archives and
the pages in [mant-roff-corpus 832e18b](https://github.com/BryanHeBY/mant-roff-corpus/tree/832e18b3645884dd1d04dc34ebd656015e5a8e71).
The fixture basenames carry a distribution prefix. The stored bytes are
otherwise identical to the upstream release members.

| Fixture | Release member | Evidence |
| --- | --- | --- |
| `freebsd-errno.2.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man2/errno.2.gz` | The 95 numbered error names bind on their original HEAD lines. |
| `freebsd-ioctl.2.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man2/ioctl.2.gz` | `FIONREAD` keeps its range and body while `int` supplies type text. |
| `freebsd-ppi.4.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man4/ppi.4.gz` | The two comma-delimited constants resolve to one description. |
| `dragonflybsd-sc.4.gz` | DragonFly BSD 6.4.2 x86-64 release ISO, `usr/share/man/man4/sc.4.gz` | The `GIO_COLOR` binding survives its pointer argument. |
| `dragonflybsd-hack.6.gz` | DragonFly BSD 6.4.2 x86-64 release ISO, `usr/share/man/man6/hack.6.gz` | `USER` and `LOGNAME` bind independently to the same login-name description. |
| `freebsd-netstat.1.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man1/netstat.1.gz` | The `netgraph` and `ng` address-family names are values of `-f`. |
| `dragonflybsd-netstat.1.gz` | DragonFly BSD 6.4.2 x86-64 release ISO, `usr/share/man/man1/netstat.1.gz` | Both address-family aliases stay values when their parenthetical annotation contains two constants. |
| `freebsd-awk.1.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man1/awk.1.gz` | Pipe operands stay in the `getline` form, with independent definitions for the three input forms. |
| `freebsd-ping.8.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man8/ping.8.gz` | The `-M` declaration keeps its ICMP mode arguments visible without indexing them as aliases. |
| `freebsd-pfilctl.8.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man8/pfilctl.8.gz` | The `link` and `unlink` forms retain their enclosed flag operands, which are not independently declared. |
| `netbsd-patch.1` | NetBSD 11.0 amd64 `man.tar.xz`, `usr/share/man/man1/patch.1` | The short and long version-control names share one original description. The two argument lists remain invocation syntax. |
| `openbsd-openssl.1` | OpenBSD 7.9 amd64 `man79.tgz`, `./usr/share/man/man1/openssl.1` | `pem` has one genuine definition and is also mentioned in many option arguments. |
| `freebsd-ng_deflate.4.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man4/ng_deflate.4.gz` | `NGM_DEFLATE_CONFIG` retains its original description alongside the parenthesized command alias. |
| `freebsd-archive_entry_acl.3.gz` | FreeBSD 15.1 amd64 `base.txz`, `usr/share/man/man3/archive_entry_acl.3.gz` | Styled permission shorthand in literal parentheses preserves both bodyless and described constant owners. |
| `netbsd-bpf.4` | NetBSD 10.1 amd64 `man.tar.xz`, `usr/share/man/man4/bpf.4` | The 28 ioctl names in 21 original definition heads bind through punctuation that reopens the native invocation. |
| `openbsd-bsd.port.mk.5` | OpenBSD 7.9 amd64 `man79.tgz`, `./usr/share/man/man5/bsd.port.mk.5` | Install templates keep their full forms and descriptions. The independently listed architecture names remain selectable. |

Each member retains its complete redistribution notices, including the
OpenSSL Project and Original SSLeay notices in `openssl(1)`. The
`awk` member includes its Lucent documentation license, also provided in
[supporting documentation](../LICENSES/LUCENT-AWK-1997.txt). The
[manifest](manifest.json) records the licenses, release archive and member hashes,
and results for all five pinned CVS profiles. Native profile output hashes
use the fixture basename from this directory under `LC_ALL=C` and `TZ=UTC`.
