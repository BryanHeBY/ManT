# Source-bound rendering audit

`RENDERING_AUDIT.json` records the 2026-09-11 audit of the fixed CVS migration:
112 reduced sources at four terminal widths, 51 licensed real fixtures, and
45,036 logical corpus inputs (37,286 distinct physical paths). This is a
rendering census with reviewed findings, **not whole-corpus acceptance**.

The reference is independently built, unpatched mandoc CVS at 2026-09-11
08:00 UTC. The record binds the producer, actual binaries, comparator versions,
source inventories and result hashes. GNU groff 1.24.1 source and rendering
provide a second implementation when investigating a difference; neither
formatter's complete presentation is an unconditional ManT contract.

## Confirmed findings and owning-layer corrections

| Finding | Source-grounded correction | Regression evidence |
| --- | --- | --- |
| Lost paragraph/fill-mode boundaries | Execute actual retained `Pp`, `fi` and `nf` requests before generic inline fallback; CVS `roff_term.c`/`mdoc_term.c`, GNU `env.cpp`/`doc-common` | `crates/mant-engine/tests/executed_flow_boundaries.rs` |
| Split combining/ZWJ glyphs and incorrect link cells | Shape complete graphemes, then project source link ranges into terminal cells; conflicting targets stay ambiguous | `mant-ui` text-flow, search, selection, navigation and actual Ratatui buffer tests |
| Literal IP keys replaced by bullets | Preserve authored heads; require named-bullet evidence. CVS `man_term.c::pre_IP` and GNU `an.tmac` print the head, not a guessed punctuation marker | `crates/mant-engine/tests/ip_literal_marks.rs`, licensed sh/GCC/rsync fixtures |
| Empty decoded table cell revived as raw escapes | Distinguish successful empty decoding from absent native payload; CVS `term_word` ignores zero-width escapes | Codec table tests and the licensed Debian `groff_me(7)` fixture |
| Root/filled/literal metadata operands leaked | Shared operand dispatch treats UC/AT as metadata and DT as presentation control; CVS `MAN_NOTEXT` and GNU `an.tmac` agree | `crates/mant-engine/tests/control_operands.rs` |

These fixes do not authorize swallowing requests inside tbl: native cell
processing can differ from ordinary document processing. Nor do retained IP
marks automatically become semantic entries: unstyled ambiguous marks remain
presentation; explicitly styled keys remain addressable terms.

## What the evidence proves

The matrix passed all 51 generated visible-content assertions, 112 basic
viewport checks and nine mutation classes. There were no process failures or
CLI source-geometry review candidates. Remaining per-case content/viewport
candidates and partial/uncovered geometry are retained in the JSON, with narrow
review explanations rather than global punctuation/whitespace waivers.

The full census reports `clean`, `partial`, `review` and `uncovered` separately.
Even `clean` means automatic dimensions were covered, not human acceptance.
The final logical counts are 54 clean, 14,355 partial, 28,659 review and 1,968
uncovered, with zero hard failures. Under identical source bytes and the same
oracle, 55 earlier review results became partial and all other status classes
were unchanged. This is not proof that all remaining differences are harmless.
The external-source filter conservatively excludes source containing include
requests; it neither executes their conditions nor counts independent aliases.
Historical manifest hashes are compared with actual decoded bytes; changed
rolling-distribution sources do not inherit previous acceptance.

Only bounded findings and the first 32 candidate artifacts are saved by the
census. They are not a severity-ranked or exhaustive human-review sample.
Reference transport, source framing, dynamic execution, tables/equations and
word-internal terminal wrapping can remain unresolved. Resource-budget exits
never count as clean. No Windows/macOS native interaction or full-corpus TUI
claim follows from this Linux run.

## Reproduce and interpret

Follow the pinned-reference audit commands in
[`docs/development.md`](../../../docs/development.md#pinned-reference-content-and-geometry-audit).
Reduced input bytes remain in the two existing layout acceptance records and
the hash-bound matrix generator; the JSON inventories every case by ID/hash.
Real fixture sources and their licenses are already checked in. Large corpus
inputs/results remain local; their recorded hashes establish identity, not
remote availability or build attestation.

Ordinary exit zero means the bounded run completed without hard failure.
`--verify` also rejects pending candidates and incomplete coverage, and is
therefore expected to reject these raw census results. Existing target,
semantic and projection ledgers were not rewritten to make this audit green.
