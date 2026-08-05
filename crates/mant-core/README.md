# mant-core

`mant-core` is ManT's renderer-independent document engine. On Unix it locates
and parses local `man` and `mdoc` sources; on every supported platform it reads the Markdown subset,
builds semantic outlines, selects excerpts, searches content, and renders
Markdown, text, and JSON projections.

Unix manual sources are parsed directly through the target-specific
`libmandoc-rs` dependency. Windows builds omit that dependency while retaining
the source-neutral document, query, search, and output layers. Process behavior
and interactive presentation remain outside this crate.

See the [ManT repository](https://github.com/BryanHeBY/ManT) for the document
protocol, CLI, MCP server, and terminal interface.
