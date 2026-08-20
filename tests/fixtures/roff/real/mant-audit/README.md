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
