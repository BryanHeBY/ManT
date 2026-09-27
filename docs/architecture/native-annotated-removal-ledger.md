# Annotated-output module and patch ledger

Status: R00 patch disposition frozen at `864aa042`; P1 deletes only the
codec-private projection named below. No row claims a vendor patch or public
`libmandoc-rs` structured API has been removed. Each later patch-removal
commit must add the exact old patch hash, proven consumers,
replacement, tests, package inventory and offline replay evidence before
marking its row complete. The active contract is
[`native-annotated-output-contract.md`](native-annotated-output-contract.md).

K = retain independently useful ability; A = adapt after a consumer moves;
D = delete after its last consumer moves; G = retain only with an identified
remaining consumer. No grep-only deletion of native safety behavior is valid.

## Module families

| Family | Disposition and owner | Deletion gate |
| --- | --- | --- |
| `libmandoc-rs` parser/AST, source bundle, raw Renderer | K: independent parser and renderer API; common session/source remains authorized-input owner | parser-only and render-only tests, package inventory |
| structured session/source/budget | A: reuse parse/render RAII, multi-source provenance, TLS, cleanup and work budgets in annotated session | include, failure recovery, next-call, concurrency |
| structured buffer/collector | A: keep active slot/token lifecycle; D: final atom/root/projection materialization | partial consume, pending/free-list, direct output and active peak |
| structured marker/structure/link/address/table | A: keep native owner/target/source/cell facts; D: display-tree reconstruction | owner/link/anchor/cell cross-layer tests |
| structured fixed/validation and placement tables | D after safe native display and consumers work | overstrike, wide/combining, table, fixed-row and size tests |
| `ffi/structured` and public structured model | A for checked ABI/transfer/error patterns; D for old result shape and facade | no stale feature/re-export, independent feature tests |
| codec roff formatter/layout/AST lowering | D after loader switch; retain source-neutral declarations, identity and targets | true loader/query/CLI/TUI, fixtures and audit input ledger |
| codec `structured_document` intermediate projection | Removed at P1 after annotated→Fixed IR and consumers became live; `native-structured` codec feature removed | no third private backend or full-body bridge; old test input disposition below |
| IR Flow content store and Markdown/TLDR | K; Fixed has one exclusive safe surface | Markdown tests and true Serde negative tests |
| UI fixed/NoWrap/selection and protocol semantics | A to final run ranges, preserve shared UI/query | viewport, click, copy, search, wire consumers |

### P1 private codec test input disposition

The removed `mandoc/projection/tests.rs` and
`mandoc/structured_document/tests{,/addresses,/scale_probe}.rs` had about 51
tests. Their old assertions about Flow blocks, atom stores, fixed placements,
and a copied `NativeProseProjection` cannot describe the exclusive Fixed
body and were deleted, not silently counted as new-path passes. The input
families have these destinations:

| Removed input family | New-path destination |
| --- | --- |
| no-fill→paragraph, `l0` table, `\z` overstrike, inline eqn | `mant-codec::annotated_fixed::tests::superseded_private_projection_display_inputs_reach_fixed_consumers`; exact CVS input rerun before assertions |
| boxed/rule/span/empty table, combining/wide overlay and long no-fill | `libmandoc-rs` annotated display/table/real-page tests plus Fixed body, renderer and TUI viewport tests; old atom/placement-specific assertions intentionally retired |
| 1,000-row allbox scale | `mant-codec::annotated_fixed::tests::thousand_row_allbox_table_keeps_two_thousand_unique_cell_regions`, using the exact old generated input and pinned CVS run; old ignored 1,000/5,000/20,000 private-projection probes are retired in favor of this new-path assertion and G1 representative-page metrics |
| source/include, heading targets, moved/repeated `.Tg`, links and identity collisions | annotated native oracle/address tests, `mant-codec::annotated_fixed` identity/link tests and `mant-ir` Fixed validation/index tests |
| C03/C04 list-kind, declaration-group, nested-entry semantic assertions | R04/R05 semantic migration gold; P1 does not claim these typed facts from a display-only result |

The old public `libmandoc-rs` structured facade and its own tests are not
part of this deletion; their removal remains the later R09 unit. This table
records input families, not a claim of one-for-one assertion equivalence.

## Current 43-patch series

| Patch | Target | Required consumer or condition |
| --- | --- | --- |
| 0001 memory UTF-8 | K | bounded input decoding |
| 0002 encoding | K | approved encoding behavior |
| 0003 continued TP/TQ | K | parser head/item behavior |
| 0004 tagged mdoc heads | K | explicit Tg preservation |
| 0005 ohash size | K | portability/type correctness |
| 0006 input traps | K | native memory/state safety |
| 0007 iterative free | K | deep-tree safe cleanup |
| 0008 renderer scratch | K | renderer/HTML buffer safety |
| 0009 optional state | K | renderer initialization |
| 0010 RFC unsigned | K | HTML byte behavior |
| 0011 direct sentinels | K | terminal safety representation |
| 0012 libbsd | K | approved platform ability |
| 0013 Pandoc fonts | K | approved font behavior |
| 0014 parser TLS | K | concurrent isolated calls |
| 0015 memory sources/budgets | K | authorized input and bounds |
| 0016 parser depth | K | recursive-depth protection |
| 0017 flow_epoch | D/G | remove AST-lowering compensation; keep minimal evidence only if reading context needs it |
| 0018 dates | K | deterministic/platform dates |
| 0019 renderer state | K | renderer/HTML state isolation |
| 0020 output/width sink | K/A | raw Renderer unchanged; add annotated sink choice |
| 0021 portable renderer | K | memory-only cross-platform rendering |
| 0022 private config | K | symbol/platform isolation |
| 0023 escape initialization | K | parser/escape safety |
| 0024 tbl escape state | D/G | remove source-recovery-only metadata after AST API audit |
| 0025 tbl source provenance | D/G | do not remove until source-backed recovery and consumers are gone |
| 0026 cell execution provenance | D/G | same as 0025, distinguish from source table |
| 0027 escape depth | K | adversarial escape bound |
| 0028 source/diagnostics | K/A | source-qualified output and diagnostics |
| 0029 terminal observer | A | retain needed node/buffer/mark events; retire old logical/projection events |
| 0030 tag origin | K/A | authored versus generated target |
| 0031 target source | K/A | declaration provenance after target move |
| 0032 cell scope | K/A | cell/source/link region facts |
| 0033 direct tbl geometry | A/D | retire duplicate geometry only after true output sink covers border glyphs |
| 0034 direct line reason | A/D | retire old fixed endline after new role mapping |
| 0035 empty cell positions | K/A | retain bounded region point, not old placement table |
| 0036 table-ready boundary | A/D | remove old scope gate only when new post-flush relation is proven |
| 0037 device-write operation labels | A | terminal letter/advance/endline evidence for the annotated sink; raw renderer bytes unchanged |
| 0038 footer body-drain boundary | A | distinguish delayed body flush from footer decoration without forcing a flush |
| 0039 tag/item display points | A | retain native boundary point even when no glyph survives; replace old inferred placement |
| 0040 post-pre region points | A | retain native region location after pre hooks without borrowing a later glyph |
| 0041 diagnostic coordinate origin | K/A | distinguish expanded diagnostic coordinates from authored source positions |
| 0042 elided paragraph boundary | K/A | retain native PP/RS presentation evidence without inventing an AST entry |
| 0043 executed word input ranges | A | synchronous consumed-byte evidence for R05 compatible links; `pos/end` remain terminal buffer units, and no word pointer crosses the callback lifetime |

Module ownership after migration: session is sole native invocation owner;
source owns authorization and source table; buffer owns active tokens, pending,
slots, free-list and partial consumption; display owns final rows/cells;
marks owns semantic range construction; result owns ABI/view/free; budget owns
fallible allocation and cumulative work. No module may mutate the buffer
state behind its complete operations. The relevant upstream execution sites
are `term.c`/`term_ascii.c`, `man_term.c`, `mdoc_term.c`, `tbl_term.c`,
`eqn_term.c`, `read.c`, `roff.c`, `tag.c` and `mdoc_validate.c`. New behavior
must follow those pinned paths, not mimic terminal output by reformatting.

Deletion order is: migrate real consumers; remove public old API/feature and
production calls; remove old code/state; remove orphaned AST/ABI fields;
rewrite/replay affected patches; verify package, license, symbol and feature
matrix. Do not prune patches in P0/P1 just because the new path has started.
