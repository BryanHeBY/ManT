# mant-ir

`mant-ir` defines ManT's normalized, source-neutral document intermediate
representation. Native man/mdoc syntax and Markdown parser events are lowered
into the same owned tree before projection, search, rendering, or interactive
navigation.

The crate provides logical document addresses; document, section, block,
inline, and semantic-entry nodes; typed identities and source ranges; quick
references; visitors; and derived indexes. It does not perform source lookup,
parsing, rendering, filesystem access, or process protocol handling.

Versioned CLI JSON and MCP contracts live in
[`mant-protocol`](https://crates.io/crates/mant-protocol); parsing and document
operations live in [`mant-core`](https://crates.io/crates/mant-core).

## License

Apache-2.0.
