# mant-sources

`mant-sources` owns `ManT`'s per-user Markdown registry and transactional source
updates. Its default feature set is read-only. The optional `update` feature
adds shallow Git acquisition and bounded HTTP archive installation for the
native `mant` CLI; MCP only consumes the local registry.

Use the default feature set in readers and services that should never acquire
remote content. Enable `update` only in a host that intentionally owns local
mutation, process execution, and network policy.

## Feature boundary

| Capability | Default | `update` feature |
| --- | ---: | ---: |
| Resolve platform data paths and parse `sources.toml` | Yes | Yes |
| Build a bounded snapshot of registered Markdown | Yes | Yes |
| Resolve exact and component-suffix document selectors | Yes | Yes |
| Shallow-clone configured Git branches | No | Yes |
| Download and extract bounded HTTP archives | No | Yes |
| Transactionally activate or prune installed sources | No | Yes |

```text
personal documents/ ────────────────┐
                                    ├─> RegisteredDocumentIndex
configured sources/<name>/ (ready) ─┘       └─> ordered, read-only lookup

sources.toml ─> update feature ─> staging ─> atomic activation
```

## Basic read-only use

One index is one filesystem/configuration snapshot. Reuse it when several
lookups must see consistent discovery results:

```rust,no_run
use mant_sources::RegisteredDocumentIndex;

let index = RegisteredDocumentIndex::load()?;
for document in index.documents() {
    println!("{} -> {}", document.logical_path, document.path.display());
}

let candidates = vec!["guides/git".to_owned(), "git".to_owned()];
if let Some(document) = index.find(&candidates, None)? {
    println!("selected {}", document.path.display());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The read-only API loads one `RegisteredDocumentIndex` snapshot, scans the root
document tree and each ready configured source once, then resolves ordered
path and component-suffix candidates without repeating filesystem discovery.
Root documents win; configured sources follow descending priority and
ascending bytewise source-name order.

With `update` enabled, Git and archive acquisition share the same staging and
atomic activation transaction. Temporary checkouts, downloads, and staging
directories are owned by an RAII workspace and cleaned on every exit path.
Provider metadata is a strict tagged value, so Git-only and archive-only fields
cannot form invalid combinations.

The update report also identifies installed directories absent from the active
configuration without deleting them. Explicit prune and dry-run operations
share the update lock, validate each directory against its recorded source
identity, and never cross into personal root documents. These mutating
operations remain outside MCP.

Upstream source trees may be recursive, and activation preserves each selected
Markdown file's relative path. Repeated leaf names in different directories
are valid; case-only logical-path collisions and an empty selection are
rejected. Platform paths, the complete `sources.toml` schema, and update
semantics are documented in the
[ManT document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md).

This crate performs no rendering, native-manual lookup, MCP transport, or
terminal work. Those responsibilities belong to `mant-engine`, `mant`, and
`mant-ui`.

## License

Apache-2.0.
