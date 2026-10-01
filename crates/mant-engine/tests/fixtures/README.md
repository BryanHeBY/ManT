# Native formatter matrix fixtures

`native_formatter_matrix.jsonl` contains 2,520 complete formatter inputs.
`native_links_matrix.jsonl` contains 868 complete link inputs. The canonical
definitions are in `scripts/roff_execution_cases.py`; quoting, historical page
headers, control placement and recovery inputs are intentionally unchanged.
Each record retains its complete source, source SHA-256, pristine profile
SHA-256 values, original lint output, native ASCII/UTF-8 physical rows and
stateful HTML href occurrences and independently decoded authored destinations.

## Regeneration

Run from the repository root after restoring the registered pristine oracle:

```sh
scripts/rebuild_reference_mandoc.sh
python3 scripts/generate_roff_execution_fixtures.py \
  --evidence target/audits/native-execution-fixtures
python3 scripts/generate_roff_execution_fixtures.py --check \
  --evidence target/audits/native-execution-fixtures-check
python3 scripts/generate_roff_execution_fixtures.py --check-sources
```

The generator checks the active registry identity in ASCII, UTF-8 and HTML
before running every exact source in ASCII, UTF-8, HTML, tree and lint. It
never invokes ManT. Complete profile output and the generation manifest stay
in the supplied directory under `target`; ordinary Rust tests read the
checked-in fixtures and need no local reference binary. Only a complete oracle
run atomically replaces either fixture. `--check` never changes gold, while
`--check-sources` needs no oracle and checks the canonical definitions and hashes.
Updating expectations from product output is forbidden.

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
