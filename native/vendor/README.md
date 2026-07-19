# Vendored native sources

## mandoc 1.14.6

The `mandoc-1.14.6` directory is extracted from the official release archive:

- URL: `https://mandoc.bsd.lv/snapshots/mandoc-1.14.6.tar.gz`
- SHA-256: `8bf0d570f01e70a6e124884088870cbed7537f36328d512909eb10cd53179d9c`

The upstream `regress/` tree is excluded because Mant maintains deterministic
source and contract fixtures in its own Rust and TypeScript test suites. The
library sources, headers, configuration probes, build metadata, documentation,
and upstream `LICENSE` are otherwise retained unchanged.

Do not patch upstream structures to expose a public ABI. Compatibility changes
belong in `mant-mandoc-sys` and its private shim so a future upstream refresh
remains reviewable.
