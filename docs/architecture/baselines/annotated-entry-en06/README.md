# EN06 real-page query review and completion record

This records the manually reviewed EN06 Fixed-query decisions and the
completed code-candidate verification. The pre-change panel, baseline
measurements, and earlier query judgments remain in
[`../annotated-entry-en00/README.md`](../annotated-entry-en00/README.md).
The measured code HEAD is `be6f099c5ae949dc30da26db99cd012587010c8d`
on `dev`, with a clean tracked worktree during the measurements. This report
is a later documentation-only commit, not a new measured binary identity.

## Source and oracle boundary

The six *unchanged, complete* repository fixtures below were decoded and
run with `target/mandoc-migration/reference/mandoc -Tutf8 -Owidth=78` on
2026-09-27. The reference establishes visible text and roff execution, while
the entry kind and evidence class are reviewed
ManT policy. Relevant pinned source paths read for this review are
`man_macro.c::blk_imp`, `man_term.c::pre_IP/pre_PP/pre_RS/pre_TP/pre_B`,
`mdoc_macro.c::blk_full`, and `mdoc_term.c::termp_it_pre`. The source SHA-256
values identify decoded roff, not compressed containers.

| Gold source | Existing fixture | Decoded SHA-256 |
| --- | --- | --- |
| F005 | `archlinux/gcc.1.gz` | `a86bd1d671aa2afff558eb3b7c8b6e2a8bbaf55cb9fb2a6549b7538dd56e0f0f` |
| F009 | `archlinux/tmux.1.gz` | `cbedf24cf75128a6794a24a4c380307f73515047945b6736933d313af4623ae7` |
| F014 | `archlinux/btrfs-subvolume.8.gz` | `7aad0096c8fa69c22b8487f45ace7e38f820d4b7dc1a5877b2e95ebd9a28ae96` |
| F020 | `fedora44/git.1.zst` | `2ee1c5dd84a69dfc91d84840e943e7df0ee06b86bf27eb4bb9369c415f3351c1` |
| F021 | `bsd-closure/dragonfly-adduser.8.gz` | `d2c2c09556f74e93bac34e9e3d17032c4f66daea96367854a096fbcc062d733e` |
| F022 | `windows-releases/rclone.1.zst` | `f35de3b3008f684a7db141a7626db68b08c46e5b3d45c97726bc6d97eebade4e` |

These are existing fixtures; no new source/license import or changed coverage
ledger is implied. Their source-specific `README.md` files retain the release,
member, transformation, and license mapping. The existing rendering,
structure, projection, fidelity, layout, and target audit ledgers remain the
records for these exact source hashes. In particular, rclone's previously
reviewed pristine-CVS layout difference is **not** reclassified as clean.

## Reviewed query decisions

Nine added queries and one independently re-reviewed existing query are in
[`ANNOTATED_FIXED_QUERY_GOLD.json`](../../../../tests/fixtures/roff/ANNOTATED_FIXED_QUERY_GOLD.json),
which now has 22 sources and 79 queries. F014 btrfs `create` is the existing
query whose expectation changed after checking its exact pinned-CVS head/body
execution and the EN04 complete-invocation rule; the other existing query
expectations were not rewritten.

| Source and query | Reviewed direct owner / provider | Negative boundary |
| --- | --- | --- |
| F005 GCC `CPATH` | One EnvironmentVariable at line 37163, own body empty; four distinct IP heads share reading context from the later paragraph. | Do not borrow that paragraph as CPATH's independent body or infer aliases. |
| F009 tmux `activity-action` | One named Term at line 4676 with its own list-item body. | Ic in session options does not by itself prove a Command. |
| F014 btrfs `create` | One Command at line 311, form `create [options] [<dest>/]<name> [[<dest2>/]<name2> ...]`, with its own TP body. | `.B` supplies font only; SUBCOMMAND context and a complete balanced invocation supply the reviewed ManT command evidence. |
| F020 Git `git-add` | One Command at line 351, form `git-add(1)`, exact scalar name binding 0..7; the direct RS body begins “Add file contents to the index.” | A SEE ALSO reference or introductory prose cannot supply this owner. |
| F020 Git `GIT_ADVICE` | No direct owner; one context mention. | Bare styled PP/RS text is insufficient for the conservative weak-declaration rule. |
| F021 adduser `random` | One named Term at line 286 with its own list-item body. | Do not infer Value without a checked local parent/group relation. |
| F021 adduser `randompass` | No direct owner; one entry-body mention under `random`. | The body `.Va` is not another definition head. |
| F022 rclone `--help` | No direct owner; 105 context mentions. | Synopsis and examples do not create a declaration. |
| F022 rclone `rclone [command] --help` | No direct owner; one context mention. | The single synopsis instruction is not a command/option definition. |
| F022 rclone `--transfers int` | No direct owner; eleven context mentions. | Its `.SS` heading is the deferred EN05 adapter case, not an EN06 entry. |

No alias groups or cross-page value domains are inferred from repeated names,
head punctuation, or nesting. The target kind of `random` and
`activity-action` was chosen *before* running the candidate gold. Source,
complete form, direct body, and reading-context witnesses are asserted in the
manifest where those facts exist; mention counts are asserted only for the
five specifically reviewed negative probes. The 105 rclone `--help` mentions
are one exact count assertion, not 105 separately reviewed queries.

The EN00 panel covered 12 real source identities and 23 page-query judgments.
The nine added EN06 queries overlap six of those judgments; adding the tmux,
rclone synopsis and rclone heading probes, plus the separate F014 btrfs
re-review, yields **14 distinct repository real source identities and 27
distinct page-query judgments** across the two records. The 79 scripted gold
queries are not being represented as 79 newly hand-reviewed judgments. These
counts do not include local-only pages absent from the repository fixture set.

## Final code-candidate gates and display boundary

The complete `env CARGO_TARGET_DIR=/home/hby/dev/ManT/target bash
scripts/check.sh --build-profile debug` run exited 0 on the measured HEAD;
the local log is `target/entry-en06-check-final-20260927.log` and ends with
`local verification succeeded`. It includes the 79/79 source-bound Fixed
query gold, workspace tests/doctests, Markdown-only and roff package tests,
all-feature `libmandoc-rs` tests, protocol schema snapshot, eight CLI
feature configurations, independent consumers, packaged-source tests,
relevant roff audits, all-feature strict workspace Clippy, all-feature
warnings-denied docs and locked fuzz compilation. Separately,
`cargo test --locked --all-features -p mant-codec -p mant-ir -p mant-protocol
-p mant-query -p mant-render -p mant-engine` passed including doctests;
the log is `target/entry-en06-related-all-features-be6f099c.log`. The
candidate's tmux
`activity-action` and adduser `random` named-Term queries are in that gold.
The separate read-only `crates/libmandoc-rs/scripts/sync-vendor --verify --archive
target/mandoc-migration/freeze-cvs-20260920T122115Z/upstream-cvs-20260920T122115Z.tar.gz`
replayed all 42 patches and reported `vendor is up-to-date`.

The release CLI was built with `cargo build --locked --release -p mant
--features annotated-preview` into the repository `target` and frozen as
`target/entry-en06-be6f099c/mant-be6f099c` (SHA-256
`aa88cd6bd5e1eaf1e0336b6607542a9431cc80f17313347e4468d8893eba5ee2`).
The behavioral reference is `target/mandoc-migration/reference/mandoc`
(SHA-256 `d7c58752e2586695d17f4a87cc5540b35ec65ebec51bbcb5de8c9e6786e7311d`).
The benchmark's frozen native executable has a *different* SHA-256,
`4892dbc7f379a956d65413b8597402f9454d38c83374188a5f0e76be93f7e88a`;
it is not substituted for that behavioral oracle.
The frozen release CLI also passed `python3
scripts/annotated_fixed_query_gold.py --cli
target/entry-en06-be6f099c/mant-be6f099c` (79/79), independently of the
debug full-gate invocation.

The four frozen performance inputs are the exact EN00 decoded files; their
SHA-256s are GCC `a86bd1d6...`, Git `2736b9d2...`, Clang `ff10a161...`,
and rclone `f35de3b3...` (full hashes in the raw benchmark report). The Git
and Clang *performance snapshots* are not the F020 and Clang gold fixtures,
which have different source hashes. For each of these four snapshots, native,
Flow and Fixed direct-text stdout length and SHA-256 exactly match that
route's EN00 result. This is a same-route before/after fidelity check, not a
claim that all three routes or pristine CVS are byte-identical. In particular,
rclone's previously reviewed pristine-CVS layout difference remains.
The separate release test
`annotated_preview_full_body_matches_real_tui_buffer_at_four_widths`
passed for all four pages at widths 20, 40, 78 and 120; it compares the
candidate's Fixed text projection with the symbols in its real TUI cell
buffer, not TUI styles or cells directly with CVS. Its log is
`target/entry-en06-full-buffers-final-be6f099c.log`; the command was
`cargo test --locked --release -p mant --features annotated-preview
annotated_preview_full_body_matches_real_tui_buffer_at_four_widths --
--ignored --nocapture` with the repository target.

## EN00 to EN06 process cost

The raw reports are `target/entry-en00.oXEsSw/run-918ae0a7/results.json`
(SHA-256 `b8d138130a13cb12e4c27107989244a1f4b4c1669cc2ea1113ab069d53f8d9a0`)
and `target/entry-en06-perf-be6f099c/results.json`
(SHA-256 `4d8998dbda666a97245b439ada8c7e90f28f5d878d5796eec6151bc5c9b601d4`).
The latter was produced by `python3 scripts/annotated_perf_baseline.py
--frozen-dir target/entry-en06-be6f099c
--out target/entry-en06-perf-be6f099c`. Both use the same Linux WSL2 host
(Intel i5-13500, glibc 2.44), `C.UTF-8`, UTC, width 78, one warm-up, 12
interleaved output rounds and eight query rounds. Figures below are medians
of fresh-process wall time and peak RSS; the raw JSON holds quartiles, CPU,
all commands and output identities. They are not isolated recognition times.

| Performance snapshot | Fixed text wall, EN00 -> EN06 | Fixed text RSS, KiB | Fixed explain wall | Fixed visible-search wall |
| --- | ---: | ---: | ---: | ---: |
| Clang | 9.81 -> 10.17 ms (+3.7%) | 10,584 -> 10,302 | 10.52 -> 10.72 ms | 10.61 -> 10.82 ms |
| GCC | 384.64 -> 393.46 ms (+2.3%) | 58,604 -> 58,260 | 410.41 -> 433.90 ms | 395.26 -> 410.61 ms |
| Git | 17.09 -> 18.17 ms (+6.3%) | 11,734 -> 11,548 | 17.91 -> 19.50 ms | 18.32 -> 19.14 ms |
| rclone | 738.17 -> 721.56 ms (-2.3%) | 144,454 -> 144,016 | 800.69 -> 777.62 ms | 770.10 -> 758.88 ms |

The four chosen Fixed explain/search JSON identities are unchanged from
EN00. The Fixed outline intentionally gains semantic entries: the Git
snapshot has 25 -> 167 entries and its outline stdout grows from 46,180 to
80,740 bytes; GCC has 3,653 -> 3,674 entries and a 0.45% outline-byte gain.
The entry counts were checked by running each frozen CLI with
`--annotated-preview --input <same frozen page> --input-format roff
--outline --outline-entries all --outline-references none --format json
--compact` and applying
`jq '[.. | objects | select(.kind? == "document-entry")] | length'`.
Thus extra semantic work is visible, but it does not explain every cost:
GCC's additional allocation calls are proportionally much larger than its
outline growth. Git/GCC Fixed wall increases are measurable in this run
(their relevant quartile ranges do not overlap); no causal attribution to
one helper or a hard performance threshold is inferred. The negative large
rclone page is stable or faster, and Fixed peak RSS did not increase here.

An additional same-source glibc public-allocator probe ran once for each
of four pages, two routes and four operations at both code baselines (32
reports each). Every candidate probe retained identical stdout, stderr and
exit status with and without interposition. Reports are under
`target/entry-a15-allocation-probe/`; the probe source SHA-256 is
`d06ec192c17578de2e4d53b460332130ef8d8513b2005aac18c8d6f9c5cbd6e8`.
The Fixed text process-wide non-`free` allocation-call / requested-byte
figures are:

| Snapshot | Calls, EN00 -> EN06 | Requested bytes, EN00 -> EN06 |
| --- | ---: | ---: |
| Git | 30,712 -> 60,232 | 7,304,968 -> 8,671,696 |
| GCC | 1,135,354 -> 1,604,654 | 209,854,738 -> 223,969,977 |
| rclone | 1,400,377 -> 1,400,543 | 383,264,101 -> 383,269,543 |

This counts public glibc allocator requests over one complete process, not
Rust-only allocations, live heap, allocation capacity, or phase cost; direct
`mmap` and custom allocators may bypass it. More short-lived work is a
plausible explanation for call growth without RSS growth, not a demonstrated
root cause. Focused follow-up targets are per-owner recognition in
`annotated_fixed.rs` and repeated checked-entry construction in
`fixed_body/evidence.rs`; any caching must retain the mutable-IR validation
boundary rather than merely suppress repeat checks.

The ignored release G1 `annotated_preview_four_page_stage_costs` test passed
on this candidate, forward and reverse page order. Its raw log is
`target/entry-en06-stage-final-be6f099c.log`; it used the same release
Cargo-test command shape with that test name. Pass 0 elapsed samples are:

| Snapshot | Native render | Lower/project | Outline | Explain | Search |
| --- | ---: | ---: | ---: | ---: | ---: |
| Clang | 6.32 ms | 1.38 ms | 0.80 ms | 1.00 ms | 0.79 ms |
| GCC | 302.06 ms | 62.62 ms | 34.99 ms | 63.86 ms | 45.10 ms |
| Git | 12.76 ms | 2.06 ms | 1.07 ms | 1.95 ms | 1.33 ms |
| rclone | 596.64 ms | 73.19 ms | 23.45 ms | 81.57 ms | 63.34 ms |

These are coarse same-process elapsed stages: lower/project includes entry
recognition and validation, and query stages can revalidate. They cannot be
subtracted from fresh-process medians to isolate recognition self-time. A
separate GDB run on the debug annotated-preview binary (SHA-256
`cf87c23d43043724aa69923d8e17e1e4f31dec09673be382f2d6517cd9fc8908`)
for the Git performance snapshot's Fixed `--explain=--help` counted 1,431
`validated_entry` calls, 317 `lexical_names` calls, 1,144 manual-call
recognition calls, three Fixed-body validations and one semantic-index build.
The raw data is `target/entry-en06-debug-be6f099c/git-fixed-explain-counts.json`.
These are current absolute debug function-entry counts, not elapsed time,
allocations or an EN00-before/EN06-after call-count comparison.

A separate 512/1,024/2,048-owner synthetic `.TP` scale probe first ran
each exact input through the pinned CVS reference. The frozen release Fixed
route then returned exactly N entries and one direct explanation of the last
name. Across seven interleaved fresh-process rounds, outline medians were
13.51/25.31/49.84 ms and explain medians 12.77/24.09/48.32 ms. The raw
record is `target/entry-en06-scale-probe/results.json` (SHA-256
`880a2bc76f72ab2cdec2f2f09be4af069021df050ae37bf16b62e8c6eeb44f7b`).
This shows no superlinear growth in that input pattern, not a general
complexity proof; focused suffix-work linearity tests also pass in the full
gate.

## Remaining scope

EN05's declaration-heading adapter and unsupported cross-page value-domain
inference remain deferred. Some Fixed responses still report
`semanticsComplete=false`; a 79/79 gold result does not assert complete
recognition of every entry on each page. No new real fixture was imported in
this unit, so the existing source/license and coverage ledgers retain their
prior scope. The default roff entry was not switched, v0.12 was not bumped,
and nothing was pushed. The measured Git/GCC allocation increase is retained
as a quantified optimization follow-up, not silently classified as clean.
