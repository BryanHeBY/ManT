# mant

`mant` is ManT's complete native document command. In a terminal it opens the
Ratatui reader; projections and explicit formats provide structured output for
agents and scripts, and `--mcp` starts the stdio MCP server.

```sh
mant git
mant gcc --outline
mant tar --explain=--exclude
mant README.md --node 1
mant git --format markdown
mant --mcp
```

Install the native CLI with:

```sh
cargo install mant --locked
```

The same executable handles interactive manuals, local Markdown, deterministic
Markdown/text/JSON output, generated schemas, and read-only MCP tools.
