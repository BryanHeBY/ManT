# mant-ui

`mant-ui` is the Ratatui frontend component used by the `mant` executable.
It renders ManT's normalized document model and owns interactive navigation,
search, scrolling, links, and terminal presentation.

The component is portable across Linux, macOS, and Windows. It is independent
of the document source: Unix callers can supply normalized man, mdoc, or
Markdown documents, while Windows callers use the same interface and complete
TUI feature set with Markdown documents without compiling libmandoc.

Install [`mant`](https://crates.io/crates/mant) for the complete command. This
component crate does not install a separate executable.
