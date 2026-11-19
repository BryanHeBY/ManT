# BSD program declaration sources

These complete gzip release members were recovered from the retained
[audit corpus](https://github.com/BryanHeBY/mant-roff-corpus/tree/832e18b3645884dd1d04dc34ebd656015e5a8e71/corpora)
on November 19, 2026. The gzip contents are unchanged. The fixture
names distinguish the releases.

| Fixture | Release archive member | Regression coverage |
| --- | --- | --- |
| `dragonflybsd-hammer.8.gz` | `usr/share/man/man8/hammer.8.gz` from DragonFly BSD 6.4.2 x86-64 | Four HAMMER version headings keep their complete forms and descriptions. |
| `dragonflybsd-limits.1.gz` | `usr/share/man/man1/limits.1.gz` from DragonFly BSD 6.4.2 x86-64 | All three usage forms bind `limits` and retain their own descriptions. |
| `freebsd-limits.1.gz` | `usr/share/man/man1/limits.1.gz` from FreeBSD 15.1 amd64 | Standalone `limits` binds before optional arguments and the documented command operand. |
| `freebsd-pkg.7.gz` | `usr/share/man/man7/pkg.7.gz` from FreeBSD 15.1 amd64 | Standalone `pkg` and contiguous `pkg add` bind their own forms. The intervening option in the bootstrap form stays within the complete form. |

The `hammer(8)` source includes The DragonFly Project’s BSD-3-Clause notice.
Both `limits(1)` fixtures retain the original David Nugent copyright
and five-condition license immediately at the start of the source. The
`pkg(7)` fixture contains its original BSD-2-Clause license and Bryan
Drewery copyright. Run `gzip -dc <fixture>` to read each embedded notice.
The original `limits(1)` work was done expressly for inclusion in FreeBSD.

[Manifest](manifest.json) records archive identities, exact gzip and
decompressed source hashes, and pristine CVS ASCII, UTF-8, HTML, tree
and lint receipts. To reproduce each receipt, run the pinned oracle with
`-T<profile> <fixture-basename>` from this directory, using `LC_ALL=C.UTF-8`
and `TZ=UTC`. The input filename is significant for lint diagnostics.

[DragonFly release archive](https://avalon.dragonflybsd.org/iso-images/dfly-x86_64-6.4.2_REL.iso.bz2) · [FreeBSD release archive](https://download.freebsd.org/releases/amd64/amd64/15.1-RELEASE/base.txz)
