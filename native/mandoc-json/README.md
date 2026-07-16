# Bundled libmandoc sidecar

`mant-mandoc-json` converts a local man-page source into the stable
`mant.roff-ast/v1` JSON protocol.  It is built from the pinned mandoc 1.14.6
source archive rather than linking the host's `libmandoc`.

Build it with:

```sh
bun run build:mandoc-json
```

For the full local verification and current-platform package, use:

```sh
bun run build
```

It runs a frozen dependency install, type check, GCC sidecar build, all tests,
and a Bun standalone build.  The resulting `dist/mant` and adjacent
`dist/mant-mandoc-json` are smoke-tested together.  This is a native Linux or
macOS build, not cross-compilation; Windows users should use WSL for now.

The build requires `curl`, a C compiler, `make`, and zlib development files.
The output is `native/bin/mant-mandoc-json`; it is intentionally ignored by
Git because it is platform-specific.  Release builds should build and package
one binary per target platform.

Inspect a real source AST with:

```sh
mant git --roff-ast
```

By default, `.so` requests are kept as `meta.aliasTarget` rather than being
expanded.  `--allow-include` enables libmandoc's source inclusion and should
only be used after the caller has enforced an allowed source-root policy.

This sidecar removes the runtime need for the system `mandoc` package.  The
normal TUI currently remains on the HTML renderer pipeline; it still uses
system `mandoc` when available until an AST-to-`SectionNode` adapter is ready.
