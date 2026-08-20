# ManT-authored audit fixtures

These small roff inputs are authored by the ManT project and distributed under
the repository's Apache-2.0 license. Unlike the adjacent attributed upstream
manuals, they exist to exercise combinations that a structural oracle must see
on every local fixture run. They are not copied from an operating system or
third-party manual.

`equation-contexts.7` combines a configuration-only `.EQ`, inline delimited
equations, a display equation, and delimiter-driven tbl cells. Focused Rust
tests remain the behavioral gate; the fixture proves that the corpus profiler
itself observes each placement class.

`projection-escapes.7` locks source entity spellings, trailing brace text in a
heading, and dollar-prefixed variable text across the native-IR-to-CommonMark
boundary.

`tq-aliases.7` and `macro-recursion.7` retain reproducible evidence for two
source-specific cases where ManT preserves more programmatic semantics than
GNU groff's terminal result: alias ownership and the complete finite document
around a recursively defined macro.
