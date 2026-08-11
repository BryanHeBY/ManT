# mant-sources

`mant-sources` owns ManT's per-user Markdown registry and transactional source
updates. Its default feature set is read-only. The optional `update` feature
adds shallow Git acquisition and bounded HTTP archive installation for the
native `mant` CLI; MCP only consumes the local registry.

This crate is an internal component of [ManT](https://github.com/BryanHeBY/ManT).
