# mantdoc upstream conformance tests

This private `mantdoc` test support validates a checksum-pinned upstream
`mandoc-1.14.6` archive without downloading, unpacking, or redistributing its
payload. It is neither part of the public parser API nor part of the published
crate.

The `examples/` commands provide explicit maintainer entry points:

```sh
cargo run --locked --package mantdoc --example mantdoc-corpus-inventory \
  -- /path/to/mandoc-1.14.6.tar.gz --m3-execution
```

`scripts/run-mantdoc-differential-shards.py` builds these private examples once
and runs the independent M3--M6 lanes concurrently. The canonical parser
snapshot and renderer comparison are intentional release-level checks, not
ordinary library tests.

All long-lived expectations live beside this support code or in focused,
source-attributed test data. Historical C-oracle migration material is not a
runtime or test dependency.
