# Native formatter matrix fixtures

`native_formatter_matrix.jsonl` contains 2,520 complete formatter inputs.
`native_links_matrix.jsonl` contains 868 complete link inputs. The canonical
definitions are in `scripts/roff/fixtures/roff_execution_cases.py`; quoting, historical page
headers, control placement and recovery inputs are intentionally unchanged.
Each record retains its complete source, source SHA-256, pristine profile
SHA-256 values, original lint output, native ASCII/UTF-8 physical rows and
stateful HTML href occurrences and independently decoded authored destinations.

## Regeneration

Run from the repository root after restoring the registered pristine oracle:

```sh
scripts/rebuild_reference_mandoc.sh
python3 -m scripts.roff.fixtures.generate_roff_execution_fixtures \
  --evidence target/audits/native-execution-fixtures
python3 -m scripts.roff.fixtures.generate_roff_execution_fixtures --check \
  --evidence target/audits/native-execution-fixtures-check
python3 -m scripts.roff.fixtures.generate_roff_execution_fixtures --check-sources
```

The generator checks the active registry identity in ASCII, UTF-8 and HTML
before running every exact source in ASCII, UTF-8, HTML, tree and lint. It
never invokes ManT. Complete profile output and the generation manifest stay
in the supplied directory under `target`; ordinary Rust tests read the
checked-in fixtures and need no local reference binary. Only a complete oracle
run atomically replaces either fixture. `--check` never changes gold, while
`--check-sources` needs no oracle and checks the canonical definitions and hashes.
Updating expectations from product output is forbidden.

## Historical families and HEAD requests

The nineteen smaller compatibility families retain complete source records in
`<family>/cases.json` beneath this directory. Their canonical definitions are
in `scripts/roff/fixtures/roff_compatibility_cases.py`; the acceptance family also reads the
immutable source files in `shared_execution_matrix/cases`.

| Family | Inputs | Main contract |
| --- | ---: | --- |
| `native_acceptance_rows` | 112 | Accepted prefixes and rejected suffixes |
| `native_control_rows` | 234 | Real control and source-row boundaries |
| `generated_word_rows` | 2,340 | Generated words share formatter execution |
| `generated_body_rows` | 134 | Generated and structural BODY handoffs |
| `kept_word_rows` | 96 | Keep, spacing and word boundaries |
| `generated_word_styles` | 48 | Styles retain generated-word ownership |
| `output_owner_rows` | 69 | Output transfers retain execution facts |
| `portable_word_rows` | 108 | Native and portable reading contracts |
| `container_word_rows` | 21 | Container pre/post and word order |
| `node_body_rows` | 310 | Actual HEAD post fitting, BODY events and edge rows |
| `section_edge_rows` | 297 | Completed empty HEAD rows and section/EOF handoffs |
| `field_spacing_rows` | 72 | Negative vertical-space debt and field padding |
| `empty_text_continuation_rows` | 8 | Empty TEXT after continued fields |
| `plain_field_rows` | 32 | Ordinary buffers, accepted invisible passes and rejected tails |
| `literal_eof_rows` | 72 | Empty literal rows at EOF and fill transitions |
| `table_control_rows` | 48 | Live column fields and real list control/post lifecycles |
| `skipped_list_heads` | 128 | Declined HEAD children have no execution side effects |
| `structural_row_handoffs` | 16 | Real nested Bl pre consumes the occupied outer row |
| `column_margin_rows` | 20 | Device padding and word origins share their live owner |

```sh
python3 -m scripts.roff.fixtures.generate_roff_compatibility_fixtures --check-sources
python3 -m scripts.roff.fixtures.generate_roff_compatibility_fixtures --check \
  --evidence target/audits/native-compatibility-fixtures-check
```

These 4,165 inputs keep each family's selected device/reading assertions.
Recovery cases, isolated table-macro fragments and the display-equation reading
contract remain explicitly identified. Wider UTF-8 recordings supplement the
original 78-column evidence; they do not overwrite it. The generator preserves
failed native HTML profiles as failures, not inferred successful output.
The Rust suites still cross real serialized JSON and their selected consumers;
a coarse accepted-content assertion does not prove column or hard-row geometry.

`native_head_vertical_requests.json` separately records 60 exact HEAD request
inputs used by `head_vertical_requests.rs`. These exercise TAG/HANG field tails,
vertical requests and target placement. They are producer/consumer records,
separate from the two large JSONL matrices and the historical family generator.
Do not reconstruct their complete sources from shortened scenario labels.

`node_body_rows` crosses TP/TQ/IP, 4/8/16-column fields, short/full-width
HEADs, empty TEXT/NBRZW/armed BACKAFTER, and br/sp/paragraph requests. It
also records section/EOF handoffs and No/Em/Lk BODY ordering in TAG/HANG.
Every fixture explicitly names its final structural boundary. Assertions
keep all leading/internal/trailing empty rows and word separators; generated
field padding widths are normalized under the existing responsive contract.
Tests separately verify the expected native HEAD/BODY AST ownership.

`plain_field_rows` keeps the ordinary paragraph and no-fill execution path
independent of definition geometry. Its 32 sources cross visible/absent
prefixes with NBRZW, combining graphs, word-end breaks, and bare/completed
zero-advance requests. Fixtures remove only the common manual margin and
retain every empty row, including leading and trailing rows; the raw native
region remains in `native_rows`. Tests cross the real query JSON string and
assert the complete physical row sequence in the native reading consumer.

`literal_eof_rows` retains every final physical row without relying on a
following heading or printable word. Its exact man/mdoc sources cross raw
blank lines, NBRZW and empty TEXT, repeated rows, visible/absent prefixes,
and EOF/fi/br-fi exits. An open table-cell fragment can receive another
column; closing the document instead must preserve an empty literal row.

`skipped_list_heads` records 128 exact sources and 128 independently recorded
empty-HEAD baselines. Pristine physical rows prove that `termp_it_pre()`
declines authored HEAD children for item/bullet/dash/enum lists; parser recovery
does not execute them. Tests assert real HEAD AST ownership and unchanged
reading rows, font styles and typed link export through JSON. Item lists also
assert exact native physical rows. Generated marker geometry keeps the existing
portable-list contract; paired invariance is not a native column-position claim.

## Assertion scopes

| Matrix | Hard rows | Accepted content | Recovery safety | Total |
| --- | ---: | ---: | ---: | ---: |
| Formatter | 1,584 | 684 | 252 | 2,520 |
| Links | 170 | 698 | 0 | 868 |

Every source crosses the actual query JSON string boundary and native plain,
legal ANSI and portable export consumers. The tests check private marker
absence and require ANSI decoration to preserve the complete plain layout.

`hard-rows` compares complete native physical rows after the fixed five-column
manual margin and one known separator belonging to the following section.
The recorded reading rows additionally apply two existing G-IND differences:
the automatic initial separator after a bare control-only filled word, and
the temporary `.ti` device origin. The latter has a separately recorded pristine
`.ti 0n` pair; row count and all non-padding output must agree before recording
it. These rules retain empty rows and authored word or trailing separators.

`accepted-content` checks exact non-whitespace accepted scalar order and each
authored destination occurrence in the typed query.
It does **not** establish terminal soft wrapping, field padding, column origins
or responsive HEAD/BODY placement. TAG/HANG/TP and column geometry in these
broad combinations has not been independently approved as a terminal layout
contract. Dedicated `head_vertical_requests`, `column_execution_boundaries`,
`native_control_rows`, render cell-layout tests and real TUI consumer tests
provide stricter physical-row and coordinate coverage; these are separate
assertions, not evidence that every broad field combination has exact geometry.

Every source exports and reparses real Markdown. Ordinary filled rich text
without authored word-end breaks or man angle-target suffixes also requires
the exact typed-occurrence inventory after reparsing. Fenced displays/tables
are literal export; generated `<br>` can enter a CommonMark HTML block; and
the man `UR` suffix can create an additional autolink. Their existing export
contract therefore covers wire/display safety rather than lossless typed
occurrences. This does not weaken typed identity assertions on the native IR.

`recovery-only` retains all invalid Ta, invalid mdoc RS, warning-producing tbl
operands and other nonzero-lint sources. Margin-character side output also
uses this scope because it contaminates heading-based artifact extraction.
These records check successful bounded production and wire/consumer safety,
not native body or geometry equivalence. Their diagnostics are never cleared
or relabeled as clean parsing.

All 868 historical link inputs retain the original header without a NAME
section. Its sole pristine `first section is not "NAME"` warning is recorded
verbatim and does not exclude the native-content or row assertions. This
exception is limited to that original one-line diagnostic; any additional
pristine diagnostic changes the scope to recovery safety.

## Upstream execution contract

The primary source is the registered pristine CVS snapshot corresponding to
`crates/libmandoc-rs/vendor/mandoc-cvs-20260927T130954Z`. `term_word()` writes
separator, BREAK, zero-width NBRZW and BACKBEFORE cells in execution order.
`term_fill()` alone decides accepted fields. `term_newln()` and `term_vspace()`
are distinct from retiring a Rust output owner. `Xo/Xc` have no native pre/post;
`termp_it_post()` owns the column BODY field flush. `termp_lk_pre()` executes
description, colon and URI once. Pure destination evidence separately invokes
the pristine HTML attribute decoder without a preceding owner's pending `\z`:
that display register can otherwise consume a byte of the stateful HTML href.
The immutable authored identity contract does not inherit that display state.
Both original HTML output and isolated destination evidence are retained.

The unpublished schema remains v0.12. Fixtures do not authorize a version bump
or a new renderer backend.
