# mant-ast

`mant-ast` defines ManT's versioned, renderer-neutral document and query
contracts. It contains the shared AST, outline, excerpt, search, tldr, and JSON
Schema types consumed by the native engine, CLI, MCP server, and terminal UI.

The crate contains data contracts only. Parsing, source lookup, projection,
and output formatting live in `mant-core`.

See the [ManT repository](https://github.com/BryanHeBY/ManT) for protocol
documentation and complete examples.
