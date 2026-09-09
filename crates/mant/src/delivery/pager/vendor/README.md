# Private minus snapshot

This directory embeds the static-output/search configuration of **minus 5.7.2**,
from <https://crates.io/crates/minus/5.7.2>, upstream
<https://github.com/AMythicDev/minus>. Crate archive SHA-256:
`1db1df1b8dd701aa57b41283b50b751b3ebc8fe1406955ec90c53b46b475fa56`.
Complete upstream MIT and Apache-2.0 licenses accompany the sources. Upstream
copyright and documentation are retained; EOF whitespace is normalized.

This is a private module, not a new published crate or public ManT API. Packaging
the implementation inside mant ensures crates.io consumers receive the fix;
a workspace-only Cargo patch would silently disappear from their build.

Mechanical adaptation qualifies upstream `crate::` paths, fixes features to
static/search on and dynamic/clipboard off, and removes upstream crate-level
documentation/lint switches. Two upstream tests are adapted for the embedding
package name and an otherwise ambiguous empty byte vector. Upstream Rust
documentation examples remain visible but are marked ignored: they import the
upstream public `minus` crate, not this private implementation. They are not
counted as executed consumer tests; native unit tests and ManT PTY tests run.
Unused public APIs
and upstream stylistic lints are allowed only at this private vendor boundary;
ManT's SGR adapter is outside that allowance and receives normal strict checks.

Logical lines propagate SGR, and wrapped physical rows call the adjacent ManT
SGR adapter before independent redraw. The replayed `../patches/0001-visible-search.patch`
also makes search/index/highlight use visible physical rows, tolerates empty
search results, and replaces escape relocation with the adjacent search-overlay
adapter. Original escapes stay at their glyph positions; match reverse-video is
reapplied across resets and removed without destroying source type colors.
Width calculation, selection and resize remain upstream-owned. No initial-width
hard wrapping or document/IR modification is performed. Native selection tests
remain enabled, including selection across soft wraps.

The replayed `../patches/0002-host-terminal-lease.patch` delegates terminal modes
to the adjacent process-owned lifecycle adapter. Partial setup, normal exit,
worker failure, panic and Unix termination share one restoration ledger; the
driver cannot independently restore those same modes. All ledger operations
take stdout first, and signal termination retains that lock until the default
signal action. Cleanup attempts every owned mode and retains failed releases
for retry. Run mode is scoped to the invocation, and the temporary panic hook
is restored before returning or resuming an unwind. Direct output acquires no
terminal modes and installs no pager hook. Worker waits observe cancellation
at bounded intervals so a peer failure cannot leave a join blocked on an idle
channel or paused-input condition variable. Interactive writes check cancellation
while holding stdout, and search cursor visibility uses the same terminal owner;
neither can restore a hidden cursor after cleanup. The existing input gate now
spans poll/read together, preventing search from stealing a reader's polled event.

Verify against an extracted, checksum-verified upstream crate:

```sh
node scripts/sync-minus-vendor.mjs /path/to/minus-5.7.2 --verify
```

The verifier specifies every adaptation and checks all source and license files.
`--patch` emits an apply_patch-compatible initial import without writing files.
Retire this embedding when a published upstream interface/fix supplies independent
styled rows, rerunning the physical-redraw and packaged-source tests first.
