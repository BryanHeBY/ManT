# Private native boundary

`mant_mandoc_shim.c` is the only compiled translation unit for the parser
bridge. It owns the document structure, parser and resolver TLS, common
allocation/diagnostic helpers, document release, and the small document
accessors. `mant_mandoc_output.c` separately owns the optional bounded output
sink and its TLS.

The files in `mant_mandoc_shim/` are included implementation fragments. Do not
compile them separately or define another copy of the session state.

| Fragment | Responsibility |
| --- | --- |
| `session.inc` | Parse entry points and the shared session setup, execution, and cleanup path. |
| `sources.inc` | Bundle and resolver reads, path validation, and Unix source authorization. |
| `render_dispatch.inc` | Optional render entry points, depth preflight, and renderer dispatch. |
| `snapshot_normalized.inc` | Borrowed node flags and normalized macro facts. |
| `snapshots.inc` | Borrowed node, tbl, and eqn snapshots. |
| `abi.inc` | C size, alignment, and field-offset probes for Rust's private view types. |

The fragments retain their original order in the translation unit. Public
bridge declarations remain in `mant_mandoc_shim.h`; splitting an implementation
must not change those declarations or the native symbol namespace.

Source authorization and parser cleanup remain available without the render
feature. Memory-only builds retain the bundle/resolver boundary while omitting
Unix file access. Render dispatch keeps its existing feature guard, including
its separate depth and output limits. Native metadata and snapshots remain
borrowed until Rust releases the one document handle.

Cargo already watches and packages the complete `shim/` directory. New
fragments therefore use the same compiler flags and packaged-source build as
the root translation unit.
