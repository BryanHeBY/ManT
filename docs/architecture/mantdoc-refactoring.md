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
| `too_many_arguments` | 40 | Introduce borrowed operation contexts for diagnostics, expansion, structural lowering, and rendering. Keep semantic inputs as named fields rather than positional tuples. |
| `too_many_lines` | 36 | Split dispatchers at grammar boundaries and move static catalogues to data tables. Do not split source-order state transitions merely to satisfy a line threshold. |

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

The first structural pass completed in order on 2026-08-27:

1. **Parser emission context — complete.** Replaced repeated
   `limits/diagnostics/truncated/source` parameter groups with a borrowed
   `EmitContext`; node, text, escape, and selected request diagnostics now
   share one accounting boundary.
2. **Expansion and replay context — complete.** Separated session-wide
   `ParserCore`, physical `SourceFrame`, and iterative `ReplayMachine` state.
   Environment, macro arguments, source coordinates, and shared budgets travel
   through explicit execution frames. Frame stacks remain iterative so
   untrusted roff input cannot consume the Rust call stack.
3. **Top-level grammar dispatch — first pass complete.** Physical control
   events classify roff/man/mdoc tokens once, mdoc grammar properties live in
   `MacroSpec`, request handlers return typed transitions, and mdoc now has
   separate event-state and root post-validation modules. The large
   source-order syntax match remains intentionally contiguous for a later
   family-by-family extraction.
4. **Renderer contexts — complete.** Terminal and HTML devices own their
   document configuration, local state, output budget, and buffer lifetime.
   `BoundedOutput` centralizes checked growth and the final complete-output
   invariant without changing node-helper dispatch.
5. **Private flags.** Use enums where states are mutually exclusive and a
   compact flag set where they are independent. Avoid changing the public AST
   only to silence a lint; measure node-size and parse/render throughput before
   and after private layout changes.

The same pass also split the roff environment into its concrete session
storage, immutable compatibility catalogue, and byte-expansion helpers. It did
not add trait objects, boxed callbacks, recursive execution, or a second
environment representation.

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

## Verification evidence

The phase gate used 12 deterministic shards and 20 workers. It passed a
737-test all-feature unit suite (736 passed, one ignored), five integration
tests, strict Clippy, the conformance manifests, and the complete renderer
lane: 659 equal, zero different, zero errors. Parser lanes retained 99 M3 cases
with 50 diagnostic cases, 276 M5 cases with 175 diagnostic cases, and 58 M6
cases with 21 diagnostic cases.

`cargo bench --locked --package mantdoc --bench compare --features render`
builds Cargo's optimized `bench` profile. On the 2026-08-27 development host it
reported median nanoseconds below; mantdoc is an in-process library call,
whereas mandoc and groff include command startup and null-device output, so
these columns describe deployment latency rather than identical APIs.

| case | bytes | mantdoc parse | mandoc lint | mantdoc render | mandoc render | groff render |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| small | 6,246 | 145,409 | 697,251 | 540,495 | 776,905 | 8,447,318 |
| medium | 62,946 | 1,320,073 | 1,683,089 | 4,876,225 | 2,665,492 | 23,789,882 |
| large | 638,946 | 16,848,143 | 12,429,691 | 55,826,156 | 22,846,170 | 178,469,038 |
| roff-macros | 18,995 | 1,199,290 | 1,830,706 | 4,786,409 | 2,708,208 | 21,564,557 |
| mdoc-inline | 77,985 | 2,138,397 | 2,304,047 | 8,375,783 | 3,931,171 | 41,255,103 |
| tbl-heavy | 15,742 | 545,645 | 1,053,731 | 2,037,262 | 1,491,133 | 35,759,887 |
| eqn-heavy | 27,338 | 1,459,677 | 2,931,141 | 3,124,942 | 3,212,664 | 49,337,631 |

The large generated parse case remains about 36% slower than mandoc lint and
the native reference renderer remains more expensive than mandoc on most
nontrivial cases. This is expected at the current boundary: mantdoc constructs
the complete owned arena plus typed diagnostics, and its renderer reconstructs
device state from that immutable arena. It remains substantially faster than
the groff CLI in every generated render case. These figures are evidence, not
a timing gate; future optimization compares the same command on the same host.
