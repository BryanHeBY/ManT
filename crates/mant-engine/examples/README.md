# Development observers

These examples expose bounded observations for repository audit tools. They are
not product query interfaces or independent pristine renderers.

| Entry | Observation |
| --- | --- |
| `measure_native_load.rs` | Load phases and selected operations for short local measurements |
| `roff_structure_profile.rs` | Owned AST obligations, IR topology and optional item census |
| `roff_semantic_profile.rs` | Entry declarations, conversions and public evidence queries |
| `roff_target_profile.rs` | Native and lowered link/reference targets |
| `roff_projection_profile.rs` | Visible projection and consumer coordinates |

An observer's private modules, tests and fixtures belong in its matching
subdirectory. `support/` contains helpers shared by examples: JSONL transport
and load/operation measurements. Private modules use ordinary functions and
explicit imports; moving code does not change an observer's schema or rules.

The structure observer deliberately parses twice: the first pass records the
owned native tree, and the second follows the normal indexed-page loader.
Its native observations, IR observations and comparison rules remain separate.
The item census reports candidates and ambiguity; it does not certify loss.

`roff_structure_profile/observer_cases.json` keeps exact pristine-derived
sources and receipts. The associated tests assert topology and deliberately
mutate observations to check detection. Maintenance must retain source/gold
identity and validate the complete JSONL response when reorganizing modules.
