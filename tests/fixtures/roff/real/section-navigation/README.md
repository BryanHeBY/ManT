# Long subsection identities from libstdc++

`archlinux-gp_hash_table.3.gz` preserves the complete gzip member
`usr/share/man/man3/__gnu_pbds::gp_hash_table.3.gz` of
[Arch gcc 16.2.1+r23+gd564253eb6c8-1](https://archive.archlinux.org/packages/g/gcc/gcc-16.2.1%2Br23%2Bgd564253eb6c8-1-x86_64.pkg.tar.zst).
The November 18, 2026 review found this page in
[mant-roff-corpus 832e18b](https://github.com/BryanHeBY/mant-roff-corpus/tree/832e18b3645884dd1d04dc34ebd656015e5a8e71/corpora/archlinux-host).
The fixture matches the local package archive byte for byte and uses a
portable basename. The [manifest](manifest.json) records hashes of the
archive, compressed member and decompressed roff, and the fixed CVS runs.

The Doxygen page follows the license of the source comments, as described
in the [libstdc++ license documentation](https://gcc.gnu.org/onlinedocs/gcc-13.4.0/libstdc%2B%2B/manual/manual/license.html).
See the [GPL license](../LICENSES/GPL-3.0-or-later.txt), the package's
[GCC Runtime Library Exception](../LICENSES/GCC-RUNTIME-LIBRARY-EXCEPTION-3.1.txt)
and the [original PBDS notices](../LICENSES/GCC-PBDS-NOTICE.txt) from
`usr/include/c++/16/ext/pb_ds/assoc_container.hpp`.

The test checks the full long title and verifies ID/path equivalence over the
page's complete outline. Fixed CVS renders the whole file and retains its
subsection HEAD and BODY boundaries.
