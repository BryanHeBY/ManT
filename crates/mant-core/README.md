# mant-core

`mant-core` is ManT's renderer-independent document engine. It locates and
parses local `man` and `mdoc` sources, reads the supported Markdown subset,
builds semantic outlines, selects excerpts, searches content, and renders
Markdown, text, and JSON projections.

Manual sources are parsed directly through `libmandoc-rs`. Process behavior
and interactive presentation remain outside this crate.

See the [ManT repository](https://github.com/BryanHeBY/ManT) for the document
protocol, CLI, MCP server, and terminal interface.
