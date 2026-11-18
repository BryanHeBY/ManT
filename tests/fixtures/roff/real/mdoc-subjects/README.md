# Combined mdoc field subjects

`illumos-kstat.3kstat` is the complete `usr/src/man/man3kstat/kstat.3kstat`
from [illumos-gate e8f5c080cc0b7997410d860afd787df30ba1cf2d](https://github.com/illumos/illumos-gate/blob/e8f5c080cc0b7997410d860afd787df30ba1cf2d/usr/src/man/man3kstat/kstat.3kstat).
The November 18, 2026 review selected the file from
[mant-roff-corpus 832e18b](https://github.com/BryanHeBY/mant-roff-corpus/tree/832e18b3645884dd1d04dc34ebd656015e5a8e71/corpora/illumos-gate-e8f5c080).
Its bytes match the retained upstream checkout. No archive extraction or
transformation was needed. The source and fixture SHA-256 is
`969d8675abc5aabfbab4954d805650e2b1ee7cb21160a497dabe2d6a36e4e101`.
The source retains its notices and uses the [CDDL-1.0 license](../LICENSES/CDDL-1.0.txt).

The fixed CVS tree places the two `.Fa` fields at line 105 under the same
`.It` HEAD. The description begins in its BODY at line 106. The test verifies
both name ranges and selects the same description by ID and path. The
[manifest](manifest.json) binds the native runs to this source.
