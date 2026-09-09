# mant-loader

Read-only local document discovery and bounded acquisition for `ManT`. The loader
resolves logical document names, explicitly supplied files and typed linked
document scopes into owned `mant-ir` content. It uses `mant-codec` for decoding
and `mant-sources` for installed Markdown registration; it does not execute
semantic queries, render reports or update sources.

## Features

The default feature set supports Markdown and installed tldr caches without
libmandoc or a C compiler. Enable `roff` to load native manuals, including
bounded gzip/zstd decompression and indexed `.so` redirects. Manual catalog
discovery itself does not require a parser. A missing native capability is not
permission to invoke a host formatter, download content or run an updater.

## Loading and ownership

`DocumentLoader` captures the host manual roots and lazily builds registered
Markdown, native-manual and unified-catalog snapshots. Reuse the same loader
when discovery and loading must share precedence; construct another to refresh
local discovery. Snapshot reuse does not freeze external file contents or make
the filesystem transactional.

For an on-demand host, prepare `PreparedCatalogQuery` before constructing the
loader. It validates filters and compiles the matcher without discovery;
`DocumentLoader::discover_prepared` then reuses that matcher and the loader's
snapshot. Applying a prepared query to an already materialized catalog also
performs no IO. Neither preparation nor reuse refreshes the host environment.

`LoadSpec` borrows an explicit logical selector or input path. It carries no
serialized query schema, search expression, output format or query view.
Loading errors are separate from the application's view-validation errors.
Only the composition layer accepts a complete request and joins loading with
semantic query execution.

```rust,no_run
use mant_loader::{DocumentLoader, LoadSpec, LoadPolicy};
use mant_protocol::InputFormat;

let loader = DocumentLoader::from_system();
let content = loader.load(
    LoadSpec::File { path: "notes.md", format: InputFormat::Markdown },
    LoadPolicy::Combined,
)?;
assert!(content.document.is_some());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Boundaries

- Registered documents and native manuals retain deterministic source priority,
  exact logical identities and explicit ambiguity. A failed logical selector
  never becomes a physical path.
- Direct inputs are separately authorized by their caller. Source-byte and
  decoded-byte limits are enforced while reading, not inferred from file size.
- Linked-document loading is bounded, breadth-first and source ordered. A
  single admission operation commits content, graph positions, traversal work
  and byte accounting together; failed candidates do not consume a document
  slot or leave half a graph node.
- Negative resolution caching is request-local and qualified by policy, source
  and manual section. A frontier is a coverage boundary, not proof that a
  document does not exist.
- Tldr cache discovery may inspect executable availability but never starts a
  program. Cache updates and host process execution belong to the application.

The source, configuration and metadata safety contracts are documented in
[the architecture](https://github.com/BryanHeBY/ManT/blob/dev/docs/architecture/native-engine.md)
and [sources guide](https://github.com/BryanHeBY/ManT/blob/dev/docs/sources.md).
ManT-authored code is Apache-2.0; optional native parsing remains in the
separately licensed and attributed `libmandoc-rs` package.
