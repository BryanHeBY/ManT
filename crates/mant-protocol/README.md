# mant-protocol

`mant-protocol` defines ManT's closed, versioned request and response contracts
for CLI JSON and MCP process boundaries. It owns schema markers, pagination,
catalog, outline, excerpt, search, and JSON Schema generation.

Normalized document content is defined separately by
[`mant-ir`](https://crates.io/crates/mant-ir). Parsing, lookup, projection, and
rendering live in [`mant-core`](https://crates.io/crates/mant-core).

## License

Apache-2.0.
