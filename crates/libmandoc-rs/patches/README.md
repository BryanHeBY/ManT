# Native patch stack

`series` is the only application order. Replay from `upstream/SOURCE` with
`scripts/sync-vendor`; Cargo compiles the checked-in result and does not apply
or select patches. Regenerate changed hunks against their exact predecessor
so formal replay needs neither fuzz nor backup files.

## Dependencies and boundaries

- Build/configuration patches precede the parser and renderer units.
- Escape-depth reporting follows the escape guard; both remain mandatory.
- Parser TLS precedes renderer TLS and output capture.
- Table row escape evidence precedes table source and cell execution evidence.
- Renderer capture precedes the memory-only platform adaptations.
- The allocation unit follows every owner/lifetime change it extends.
- Safety, session isolation, input budgets and owned execution evidence remain
  baseline requirements; disabling native rendering does not remove them.

Patch categories describe responsibility. They do not claim upstream submission
or acceptance and do not turn a no-feature build into the pristine oracle.

## Compile-time compatibility

All thirty patches are always replayed. Patch `0021` gates only the `libbsd`
catalogue entry with `MANT_MANDOC_COMPAT_LIBBSD`; `0022` gates only the Pandoc
font aliases with `MANT_MANDOC_COMPAT_PANDOC`. Encoding recovery in `0020`
remains a baseline correctness fix.

Patch `0019` always retains complete-token equation evidence. The owned-view
shim gates its GNU compatibility admission with `MANT_MANDOC_COMPAT_GNU_EQN`,
and the Rust projection also checks the Cargo feature when reading public values.
No feature changes native or private-view structure layout. Rendering sources
are selected as a group by the existing `MANT_MANDOC_RENDER` build control.

## Number mapping

The previous names below refer to commit `343ba431`. `Historical-Replaces`
inside patch headers instead records older superseded stacks and is not a
reference to either column in this table.

| Current patch | Previous patch |
|---|---|
| `0001-keep-ohash-size-unsigned.patch` | `0005-keep-ohash-size-unsigned.patch` |
| `0002-apply-private-config-to-roff-escapes.patch` | `0022-apply-private-config-to-roff-escapes.patch` |
| `0003-initialize-escape-parser-state.patch` | `0023-initialize-escape-parser-state.patch` |
| `0004-bound-memory-input-utf8.patch` | `0001-bound-memory-input-utf8.patch` |
| `0005-preserve-continued-tp-aliases.patch` | `0003-preserve-continued-tp-aliases.patch` |
| `0006-retain-already-tagged-mdoc-heads.patch` | `0004-retain-already-tagged-mdoc-heads.patch` |
| `0007-replace-input-traps.patch` | `0006-replace-input-traps.patch` |
| `0008-free-native-trees-iteratively.patch` | `0007-free-native-trees-iteratively.patch` |
| `0009-bound-escape-parser-depth.patch` | `0027-bound-escape-parser-depth.patch` |
| `0010-report-escape-depth-truncation.patch` | `0029-report-escape-depth-truncation.patch` |
| `0011-isolate-parser-session-state.patch` | `0014-isolate-parser-session-state.patch` |
| `0012-memory-sources-and-input-budgets.patch` | `0015-memory-sources-and-input-budgets.patch` |
| `0013-bound-native-parser-depth.patch` | `0016-bound-native-parser-depth.patch` |
| `0014-deterministic-manual-dates.patch` | `0018-deterministic-manual-dates.patch` |
| `0015-retain-executed-flow-boundaries.patch` | `0017-retain-executed-flow-boundaries.patch` |
| `0016-retain-executed-tbl-escape-state.patch` | `0024-retain-executed-tbl-escape-state.patch` |
| `0017-retain-tbl-source-provenance.patch` | `0025-retain-tbl-source-provenance.patch` |
| `0018-track-tbl-cell-execution-provenance.patch` | `0026-track-tbl-cell-execution-provenance.patch` |
| `0019-retain-eqn-ldots-token-eligibility.patch` | `0028-retain-eqn-ldots-token-eligibility.patch` |
| `0020-preserve-unknown-encoding.patch` | `0002-preserve-unknown-encoding.patch` |
| `0021-libbsd-library-name.patch` | `0012-libbsd-library-name.patch` |
| `0022-pandoc-verbatim-fonts.patch` | `0013-pandoc-verbatim-fonts.patch` |
| `0023-size-renderer-scratch-buffers.patch` | `0008-size-renderer-scratch-buffers.patch` |
| `0024-initialize-renderer-optional-state.patch` | `0009-initialize-renderer-optional-state.patch` |
| `0025-keep-rfc-url-bytes-unsigned.patch` | `0010-keep-rfc-url-bytes-unsigned.patch` |
| `0026-render-direct-layout-sentinels.patch` | `0011-render-direct-layout-sentinels.patch` |
| `0027-isolate-reference-renderer-state.patch` | `0019-isolate-reference-renderer-state.patch` |
| `0028-capture-deterministic-reference-output.patch` | `0020-capture-deterministic-reference-output.patch` |
| `0029-portable-memory-renderers.patch` | `0021-portable-memory-renderers.patch` |
| `0030-grow-native-text-append-runs.patch` | `0030-grow-native-text-append-runs.patch` |
