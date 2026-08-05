# mant-ui

`mant-ui` is the in-progress Ratatui frontend for ManT. It consumes the same
renderer-neutral `mant-ast` query bundle as the existing OpenTUI application.

During the migration it builds the temporary `mantui-rs` executable so the new
frontend can be tested without changing the released `mant` command. Once the
interactive behavior reaches parity, this crate will become the default mode
of the single `mant` binary.

Run the current development frontend with:

```sh
cargo run --manifest-path engine/Cargo.toml -p mant-ui --bin mantui-rs -- git
```
