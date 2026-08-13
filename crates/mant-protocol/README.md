# mant-protocol

`mant-protocol` defines ManT's closed, versioned request and response contracts
for CLI JSON and MCP process boundaries. It owns schema markers, pagination,
catalog, outline, excerpt, search, and JSON Schema generation.

Normalized document content is defined separately by
[`mant-ir`](https://crates.io/crates/mant-ir). Parsing, lookup, projection, and
rendering live in [`mant-engine`](https://crates.io/crates/mant-engine).
The complete wire contract is documented by
[`mant-protocol(5)`](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-protocol.md).

## License

Apache-2.0.
