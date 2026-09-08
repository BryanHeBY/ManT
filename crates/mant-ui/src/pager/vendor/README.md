# Private minus snapshot

This directory embeds the static-output/search configuration of **minus 5.7.2**,
from <https://crates.io/crates/minus/5.7.2>, upstream
<https://github.com/AMythicDev/minus>. Crate archive SHA-256:
`1db1df1b8dd701aa57b41283b50b751b3ebc8fe1406955ec90c53b46b475fa56`.
Complete upstream MIT and Apache-2.0 licenses accompany the sources. Upstream
copyright and documentation are retained; EOF whitespace is normalized.

This is a private module, not a new published crate or public ManT API. Packaging
the implementation inside mant-ui ensures crates.io consumers receive the fix;
a workspace-only Cargo patch would silently disappear from their build.

Mechanical adaptation qualifies upstream `crate::` paths, fixes features to
static/search on and dynamic/clipboard off, and removes upstream crate-level
documentation/lint switches. Two upstream tests are adapted for the embedding
package name and an otherwise ambiguous empty byte vector. Unused public APIs
and upstream stylistic lints are allowed only at this private vendor boundary;
ManT's SGR adapter is outside that allowance and receives normal strict checks.

Behavioral delta is confined to `screen/mod.rs`: logical lines propagate SGR,
and wrapped physical rows call the adjacent ManT SGR adapter before search and
independent redraw. Original logical text, width calculation, search, selection,
terminal lifecycle and resize mechanisms remain upstream-owned. No initial-width
hard wrapping or document/IR modification is performed. Native selection tests
remain enabled, including selection across soft wraps.

Verify against an extracted, checksum-verified upstream crate:

```sh
node scripts/sync-minus-vendor.mjs /path/to/minus-5.7.2 --verify
```

The verifier specifies every adaptation and checks all source and license files.
`--patch` emits an apply_patch-compatible initial import without writing files.
Retire this embedding when a published upstream interface/fix supplies independent
styled rows, rerunning the physical-redraw and packaged-source tests first.
