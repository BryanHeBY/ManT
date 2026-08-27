# Mantdoc refactoring audit

This audit records structural work that remains after native compatibility was
reached. It is deliberately separate from the compatibility milestone log:
refactoring must preserve the canonical AST, diagnostics, engine IR, and
reference renderer contracts while improving maintainability and throughput.

## Clippy policy

`mantdoc` treats `clippy::all`, `clippy::pedantic`, and selected high-signal
nursery lints as errors in its focused work loop. Local Clippy exceptions are
limited to three structural categories and their checked-in count may only
decrease:

| lint | current ceiling | removal strategy |
| --- | ---: | --- |
| `struct_excessive_bools` | 4 | Replace private independent booleans with typed state or compact flags. Preserve named public `NodeFlags` fields until the public AST compatibility decision is explicit. |
| `too_many_arguments` | 47 | Introduce borrowed operation contexts for diagnostics, expansion, structural lowering, and rendering. Keep semantic inputs as named fields rather than positional tuples. |
| `too_many_lines` | 37 | Split dispatchers at grammar boundaries and move static catalogues to data tables. Do not split source-order state transitions merely to satisfy a line threshold. |

All correctness, ownership, allocation, byte-counting, numeric-conversion, and
match-arm exceptions have been removed. The focused work loop and canonical
workspace/CI check both run `scripts/check-mantdoc-clippy-exceptions.sh`, which
rejects a new exception category or an increase in any remaining category.

The complete `clippy::nursery` group is not a suitable hard gate: in this
crate it is dominated by `redundant_pub_crate` for intentionally private
modules and `missing_const_for_fn`, and it includes suggestions that can erase
deliberate parallel collection in tests. High-signal nursery lints are enabled
individually instead.

## Refactoring order

1. **Parser emission context.** Replace repeated
   `limits/diagnostics/truncated/source` parameter groups with a borrowed
   diagnostic sink and a bounded node emitter. This removes most exceptions
   in `parser/emit.rs` and makes budget accounting impossible to omit.
2. **Expansion and replay context.** Consolidate environment, macro arguments,
   source coordinates, and shared budgets into explicit execution frames.
   Keep frame stacks iterative so untrusted roff input cannot consume the Rust
   call stack.
3. **Top-level grammar dispatch.** Split the 4,200-line parser driver and the
   3,500-line mdoc driver by request family. Each handler should return a typed
   transition (`consumed`, `replay`, `open`, `close`, or `continue`) rather
   than mutating unrelated loop state.
4. **Renderer contexts.** Bundle bounded output, width, font, indentation, and
   table geometry into device-specific writers. Convert pinned character and
   library-name catalogues from long match functions to sorted static data
   tables with focused lookup tests.
5. **Private flags.** Use enums where states are mutually exclusive and a
   compact flag set where they are independent. Avoid changing the public AST
   only to silence a lint; measure node-size and parse/render throughput before
   and after private layout changes.

## Performance rules

- Reuse the arena and borrowed context objects; do not introduce per-node
  trait objects or boxed callbacks.
- Prefer `memchr` for byte searches/counts and checked `num-traits`
  conversions at the compatibility boundary. Both were already present in the
  resolved dependency graph, so making them direct dependencies adds no new
  package to the lockfile.
- Preserve iterative stacks for nested scopes, tables, equations, and render
  traversal.
- Benchmark before merging a structural phase and run the parallel canonical,
  IR, and renderer differential lanes after it.
