# mant-sources

`mant-sources` owns ManT's per-user Markdown registry and transactional source
updates. Its default feature set is read-only. The optional `update` feature
adds shallow Git acquisition and bounded HTTP archive installation for the
native `mant` CLI; MCP only consumes the local registry.

The read-only API loads one `RegisteredDocumentIndex` snapshot, scans the root
document directory and each ready configured source once, then resolves ordered
name candidates without repeating filesystem discovery. Root documents win;
configured sources follow descending priority and ascending bytewise
source-name order.

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

Upstream source trees may be recursive, but activation flattens selected
Markdown into one directory and rejects public-name collisions or an empty
selection. Platform paths, the complete `sources.toml` schema, and update
semantics are documented in the
[ManT document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md).

This crate performs no rendering, native-manual lookup, MCP transport, or
terminal work. Those responsibilities belong to `mant-core`, `mant`, and
`mant-ui`.

## License

Apache-2.0.
