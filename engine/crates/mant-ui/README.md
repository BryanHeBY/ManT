# mant-ui

`mant-ui` is the feature-complete Ratatui migration candidate for ManT. It
consumes the same renderer-neutral `mant-ast` query bundle as the existing
OpenTUI application and keeps parsing, projection, and tldr lookup inside the
shared Rust engine.

During the migration it builds the temporary `mantui-rs` executable so the new
frontend can be tested without changing the released `mant` command. The
frontend now covers the parity surface needed before repository consolidation:

- structured blocks, inline styles, exact roff spacing, tables, and tldr panels;
- hierarchical and semantic-option navigation with folding and scroll sync;
- confirmed full-document search with exact wrapped-row highlighting;
- keyboard menus, help, page-local links, and terminal-safe shutdown;
- mouse navigation, menus, links, search cursor placement, wheel scrolling,
  resizable boundaries, and draggable navigation/content scrollbars.

Unit tests use Ratatui's deterministic test backend. Integration tests lower
every real roff fixture in the repository at narrow, normal, and wide terminal
widths, and both shipped Markdown manuals use the same rendering pipeline. The
workspace CI also runs a packaged executable smoke test. Directory layout and
the final `mant` binary merge are intentionally deferred to the next refactor.

Run the current development frontend with:

```sh
cargo run --manifest-path engine/Cargo.toml -p mant-ui --bin mantui-rs -- git
```
