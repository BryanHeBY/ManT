# mant

`mant` is ManT's native command-line interface and stdio MCP server. It turns
local Unix manual pages and Markdown documents into structured outlines,
targeted excerpts, semantic option explanations, location-aware search
results, and Markdown, text, or JSON output.

```sh
mant gcc --outline
mant tar --explain=--exclude
mant README.md --node 1
mant --mcp
```

Install the native CLI with:

```sh
cargo install mant --locked
```

The separate `mantui` executable provides ManT's interactive terminal reader
and is distributed through the
[ManT releases](https://github.com/BryanHeBY/ManT/releases).
