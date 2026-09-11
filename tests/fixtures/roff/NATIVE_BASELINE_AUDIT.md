# Native baseline audit

Companion record: [NATIVE_BASELINE_AUDIT.json](NATIVE_BASELINE_AUDIT.json).
This record covers the mandoc 1.14.6 → fixed CVS 2026-09-11 migration.
Local `scripts/check.sh` completed successfully at the recorded final producer,
including both new standard-specifier compatibility tests. Remote CI and release
publication are separate obligations.

## Coverage and results

The restored manifest contains 45,036 rows, representing 37,286 physical source
paths. All sources were readable and decompressible. Actual decoded hashes
match the historical manifest for 36,584 rows; 8,452 Arch host sources changed.
Consequently, this is a hash-bound current-state audit, not an exact replay of
the historical corpus. Physical and decoded hashes have separate meanings.

| Check | Result |
| --- | --- |
| Target ownership | 45,036 clean after independent oracle corrections and full reruns |
| Semantic production input | 44,178 clean; 30 preexisting review; 828 unsupported standalone aliases |
| Canonical CLI comparison | 20,416 unchanged; 14,783 diagnostics-only; 9,009 document changes; 828 identical failures |
| Changed-document body text | 7,112 of 7,689 unique documents exact; 577 changed, covering 61 reviewed delta classes |
| Checked-in structure fixtures | 51 clean |
| Checked-in fidelity/layout fixtures | 5 fidelity and 1 layout review, all reproduced with the old CLI and reviewed against source |

All 21 initially undiscoverable `.roff` generator sources received explicit
same-byte supplements. All 828 semantic/CLI exceptions contain only `.so`
redirects, deliberately unsupported by direct `--input`: neither parser crashes
nor successful semantic checks. The JSON lists every exception by logical ID
and actual source hash.

## Interpreting differences

Target oracle fixes addressed escaped spelling, spacing and token boundaries;
product matching tolerance was not broadened. All 102 semantic groups on the
30 review pages have the same members in the old CLI. They remain
`preexisting-review`, not clean.

The 61 body-token classes map to eight source-reviewed native-behavior families,
including `.Ux`, `.Lb`, literal escapes and malformed string interpolation.
Upstream deliberately removed `.St -xsh4.2` in `st.c` revision 1.17; use portable
`.St -xpg4.2`. No local alias restores the old behavior.

Only three declared JSON pointers were ignored. Diagnostics, metadata, links,
identities, structure and ordering remain evidence; body equality does not
approve every structural change. Diagnostic summaries truncate 149 paths.
Full per-side hashes remain recorded for replay.

The five fidelity candidates reproduce the old CLI's full plain output and
match existing source-hash reviews. All 14 rclone layout candidates have intact
no-fill occurrences; global matching against other flowed prose caused them.

## Replay

1. Restore the manifest/corpus and verify hashes. Relocate physical paths without
   changing logical IDs; separately record changed or unavailable source bytes.
2. Preserve recorded old/current executable hashes and source diffs. A different
   build is a new producer, not automatically the binary audited here.
3. Run `scripts/audit-roff-targets.py` and `scripts/audit-roff-semantics.py` with
   explicit manual roots and new output ledgers under `target`. Preserve the
   generator and alias handling above; local orchestration scripts are hashed.
4. Compare old/current `--input PATH --input-format roff --format json --compact`
   outputs, including separate diagnostics. Review differences before approval.
5. Run `scripts/audit-roff-fidelity.py` and `scripts/audit-roff-layout.py` on the
   fixtures against a pristine renderer pinned by `crates/libmandoc-rs/upstream/SOURCE`.
   Use an explicit reference identity and new ledgers; preserve old provenance.

The compact record distributes hashes and conclusions, not the external corpus
or local output archives. Included-file dependency graphs were not independently
frozen. This Linux run does not establish other-platform behavior.

## Cross-platform follow-up

The recorded full-corpus producer contains 22 patches. Subsequent Windows CI
identified MSVC C4701 diagnostics in the new escape parser; patch 0023 explicitly
initializes optional argument/recursive-offset state without replacing any of
the existing assignments. Parser and reference-renderer regressions cover valid
special escapes, recursive expansion, invalid delimiters and incomplete escapes.
The original corpus hashes and producer records above remain unchanged; they
are not relabeled as results from this follow-up build.

The clean follow-up producer `8d06257703564385225e4b7d2fd810cde878e08f`
was independently rerun over all 45,036 target inputs. Its profile binary SHA-256
is `effb82f84395c743bb640e1e4ec964ab46db10cb1cab41f121ec3a26edb974bc`.
All results were clean and matched the original producer's records byte for
byte (including the 21 generator supplements compared by complete findings).
The main result-stream SHA-256 remains
`de8a57659541e6ddfe3c3fcf063f4e2023b44c6677866b53d57ad3fb7698d615`.
Subsequent Windows packaging verification also required LF checkout for the
distributed upstream license copy; the strict byte-for-byte license assertion
was retained rather than normalizing away distribution differences.
