# Escape grammar corpus

`escape_rules.json` freezes **2,428 execution identities / 2,271 distinct
sources** against the registered pristine CVS oracle. The same syntax can have
separate opening and closing delimiter identities; this does not increase the
distinct-source count.

Generate sources with `scripts.roff.fixtures.escape_rule_cases.cases()`. Record
gold only with:

```sh
python3 -m scripts.roff.fixtures.record_escape_rule_matrix --evidence target/audits/escape-rules
```

Use `--check` to rerun all five oracle profiles without rewriting the fixture.
The source-only checks in `scripts.roff.tests.test_escape_rule_fixtures` require
no local oracle. They verify exact sources and all required core tuples.

## Rules and execution reachability

| Rule | Identities | Native branch / reachable input | Permanent assertion |
| --- | ---: | --- | --- |
| E01 | 1032 | `roff_escape_impl()` size sign, legacy digit count, counted/bracketed/fixed quote, copy escape, nested skip, empty/missing/unclosed | Owned AST → IR → actual JSON text/row/word comparison; scalar payload/consumed-end/completion checks |
| E02 | 729 | Standard arguments for F/M/Y/k/m/f, including empty names; O1/O2 and diagnostic O/unknown controls; trigger identity at both delimiters | Same comparison, with separate metadata for all 324 normal-shape core tuples |
| E03 | 376 | o/X/Z/C/h/N × six delimiter spellings × closed/opening-only/unclosed × No/Lk | Same comparison for native-equivalent projection; N source-recovery spellings separately qualified by acceptance replay |
| E04 | 96 | Depth 0–3 with size/ignore/named/undefined inner escapes, including inner literal quotes | Same comparison; no recursive Rust call stack |
| E05 | 120 | Five pending text states × four following owners/boundaries × three outers × No/Lk | Same comparison; IGNORE's NBRZW participates in row execution without consuming a pending glyph |
| E06 | 48 | man TEXT/B/I/BR/BI/UR, mdoc No/Em/Sy/Lk/definition HEAD/literal × four canonical words | Same owned/JSON comparison; acceptance replay also exercises the actual CLI |
| E07 | 24 | Depth 1/2/8/64/255/256/257/300, closed/unclosed; complete and failed 1K/2K/4K/8K payloads | Task/index work counters, bounded stack, explicit budget status; protected native loss is covered by completeness tests |
| RC01/RC02 | 3 | Independently named original findings | Always retained independently of generated tuples |

Every source runs ASCII, UTF-8, HTML, tree and lint **before** behavioral gold is
written. Full raw output is retained in the chosen evidence directory; the
fixture binds exact source, profile exit codes and stdout/stderr hashes. A lint
warning is admission evidence, not permission to omit body assertions.

The core size and standard-name axes are full Cartesian products. E03/E04/E05
also use full products, so all their pairwise projections and shared-state
triples are included. E06 deliberately crosses every carrier with the four
canonical grammar words. EXPAND strings/registers are processed by native roff
before ordinary owned TEXT exists; scanner support for remaining expansion
syntax is defensive, not a claim that those parser states are produced here.
Sizes have fixed literal quotes; an arbitrary escaped size delimiter is not a
reachable size shape. No substituted spelling is counted as that shape.

## Projection and coverage boundaries

The Rust row helper compares content and separator existence without promising
native generated padding columns. It retains every leading, interior and
trailing row. It does not fold NBSP, glyphs, or empty rows into ordinary spaces.
The corpus has no authored indentation in its compared text samples.

Twenty-four numbered-character opening/unclosed cases retain ManT's documented
source-spelling recovery rather than native ERROR's invisible cell. Four depth
257/300 cases compare protected output and content-loss diagnostics through the
completeness corpus; pristine has no project limit. Neither is silently dropped
from the frozen source inventory or the shared acceptance replay.

`escape_policies.json` binds those 24 recovery cases separately to exact source,
native UTF-8/tree output, and the registered pristine binary. Its readable rows
are derived from the published recovery contract and authored syntax. They are
not product snapshots. Policy validation rejects changed source tails, quotes,
completion, reference identity, and output hashes; closed valid numbered glyphs
never match. The policy grants only its three stated text/row/word axes.

E02 also contains 81 font postclassification sources. Their AFTER glyph masks
are observed from pristine device backspace overlays and asserted after real
owned lowering and JSON string readback. They cover complete, missing, unclosed
and invalid names across TEXT/B/I. Twelve exact sources use the existing
Pandoc C/V/VB/VI font extension (native patch 0013 and the current manual),
with original native masks retained and a source/profile-bound policy derived
from that declared font mapping. All other masks are compared directly. E01/E02/E03 append-only boundary cohorts
separate mandatory EOF, unclosed payload and standard shape opening extent;
all original 1,923 identities remain unchanged.

Font bits outside that explicit cohort, semantic identity/source ranges,
Markdown readback and actual TUI buffers are separate axes. Their dedicated tests and acceptance ledger must
report coverage independently; grammar row equality does not assert them.
