# BSD closure fixtures

These four manuals are the minimal real pages promoted from the final BSD
release audit. Their complete permissive license notices remain embedded in
the source bytes.

| Fixture | Official release artifact and member | Artifact identity | Fixture identity | Regression |
| --- | --- | --- | --- | --- |
| `netbsd-drm.4` | NetBSD 11.0 amd64 `binary/sets/man.tar.xz`, `usr/share/man/man4/drm.4` | SHA-512 `1a0d59ae43d92e1880ea54caae321e3ee5e59d39145a1fbd2a1d700e2b5be63759b0a4df55f9d1834707e48934267b8e77a29878717a7e7085fb7e3c8fbdae33` | Exact decompressed SHA-256 `de68d037313c80fce7ecdb72da76e09e6009aafba7ad6673d1a6e7df7c6ed9a3` | Decode the deployed `\[vc]` spelling in `Jaromír Doleček` instead of leaking the raw escape. |
| `dragonfly-adduser.8.gz` | DragonFly BSD `dfly-x86_64-6.4.2_REL.iso.bz2`, `usr/share/man/man8/adduser.8.gz` | Official MD5 `906095312c4a4ac0577fb91d5eb87033`; locally recorded SHA-256 `373150a21eeb7ce0f20c7faf1b8129145bf3bf0463a45d0dc18aad274f7ed661` | Original gzip SHA-256 `f146029824cb45903197ba49c0562a83c40ba392f706f749662b4e16071a1abf`; decompressed SHA-256 `d2c2c09556f74e93bac34e9e3d17032c4f66daea96367854a096fbcc062d733e` | Carry an outer `.Sm off` state into a `.D1` display so the documented colon-delimited account record remains exact. |
| `dragonfly-gdb.1.gz` | DragonFly BSD `dfly-x86_64-6.4.2_REL.iso.bz2`, `usr/share/man/man1/gdb.1.gz` | Official MD5 `906095312c4a4ac0577fb91d5eb87033`; locally recorded SHA-256 `373150a21eeb7ce0f20c7faf1b8129145bf3bf0463a45d0dc18aad274f7ed661` | Original gzip SHA-256 `bf42f6da0a5380e947f4033dd63b39d844af9e68ecd1ae6f3082092b999c4d89`; decompressed SHA-256 `249b232d42ce6091c61bf1ab85e0e32e9aeb1c30ab73f9ba0d8ef943a55200d2` | Preserve consecutive `.TP` option aliases whose font macro ends in a `\\c` continuation. |
| `openbsd-current-term.5` | OpenBSD amd64 snapshot `man80.tgz`, `usr/share/man/man5/term.5`; build `Sun Aug 23 04:21:40 UTC 2026` | Official SHA-256 `213189e7db8c7cc33bcbb29d3cc87f653d3392ee1c873625c62da87415192512` | Exact decompressed SHA-256 `e756a45cd89d0afef51fde45de85fc69030c2ec151e8937fc6dc5d86886002bf` | Parse `\\s-20000` as signed legacy size `-2` followed by the visible hexadecimal offset `0000`. |

The NetBSD set came from
`https://cdn.netbsd.org/pub/NetBSD/NetBSD-11.0/amd64/binary/sets/`.
The DragonFly image came from
`https://avalon.dragonflybsd.org/iso-images/`, and the rolling OpenBSD manual
set came from `https://cdn.openbsd.org/pub/OpenBSD/snapshots/amd64/`. All were
retrieved on 2026-08-24 and verified before extraction. No source
transformation was applied: the NetBSD and OpenBSD members are stored as their
exact uncompressed bytes and the DragonFly members retain the gzip bytes
shipped on the ISO.
