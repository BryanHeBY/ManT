# P1/G1 annotated-preview candidate evidence (2026-09-24)

This is the first true CLI/Fixed-IR comparison against the frozen P0 old-path
baseline in [`../native-annotated-output-p0/README.md`](../native-annotated-output-p0/README.md).
It is not the R08 production switch. The default roff route remains unchanged;
every new-path command used `--annotated-preview` and explicit roff input.
The pinned pristine CVS reference and 78-column profile are the P0 oracle.

## Four complete results

The release CLI was built with `cargo build --locked --release -p mant
--features annotated-preview`. Each path below is relative to the repository
root. Query terms were `-x`, `--help`, `-help`, and `--help`, respectively.
The hashes include the complete direct text result or compact JSON response,
not just the viewport sample. The raw CVS byte hashes and decoded input
hashes are recorded in P0 and were not redefined here.

| Page / fixture suffix | Native final rows / output bytes / runs | Headings / owners / anchors | Direct text SHA-256 | Outline / explain / visible-search SHA-256 |
| --- | ---: | ---: | --- | --- |
| GCC / `archlinux/gcc.1.gz` | 31,888 / 1,437,094 / 224,167 | 30 / 5,065 / 4,557 | `d158c1cef52fcef9c7da4bd46eb96ecc9234d8d473c480e7eb13535681e3e752` | `d4dc847aebe1d50b100ca0dca85886e6efede4199bfe2eb32c5b0546722eecb6` / `9bc491e23709b9e74040917acca25ee7eabc7dc7a06c311518e7b52df39833f8` / `5bd6ffe49c58bde9e1c9b6f1f2ec7d87bd131e1b49efd9bbf449a9f429880e91` |
| Git / `archlinux/git.1.gz` | 1,651 / 62,569 / 10,084 | 37 / 7 / 0 | `5bf53ec881555cdd0de7b6fe29b33c1c591fc87b021860b6038a11b0a4fd7474` | `468c90300963845854929ceb71d3471cf63c75303ce83f609f259bb1c4b7b0b0` / `fd34c972872d38685e065f6189ac0cef2115006a74b21b596b4811f5ecb89e99` / `22309e980c5c7d5ea28b5a64bebfd3bcd52f88cab2d0573178c9e3b3e9481510` |
| Clang / `archlinux/clang.1.gz` | 676 / 26,573 / 3,955 | 16 / 92 / 88 | `9b5f021a9617a4659fa2da07139bd0b3d60791748b346c7af82a2e55b190346c` | `001d2aadae2b85b2ad74ce8435a18dfc818cb045df87c180290c6196d1b769eb` / `8bc917aacf971af5ee5bcadb7b626eee7cf8c6b5543823036fbe751299435b69` / `8e1526a7bcdbb49fb96828244e681e8ee171bbdcd85e7eeed6c967f2317879eb` |
| rclone / `windows-releases/rclone.1.zst` | 90,871 / 3,257,887 / 503,339 | 3,430 / 18,451 / 0 | `f5a15f6e0f65dac4f832e9c9418a75dd13ae81142b99941238677412182b9323` | `536a6449749231cc7b00436bf3fbef0e368e36f27e14b47f34f57fe0a3be2185` / `6d99b8b7eaa4a4677d5b6e49e13002c5f4b71b27f8e6f7c3293da7f956007dc8` / `6b87c73ba58e8436bda38c8f2d4e2c4d2b80c546896012a8c76adeed315a2986` |

The release `annotated_real_pages` native-only test reported final marks /
selection parts / join bytes: GCC `19,863 / 198,222 / 10,446`, Git
`156 / 8,854 / 413`, Clang `420 / 3,436 / 194`, and rclone
`70,759 / 423,179 / 10,316`. These are final sidecar counts, not a
historical token trace or a peak-allocation measurement.

All four had zero link marks in these particular pages; separate small
native/IR/UI tests cover link coordinates and activation. The literal
Markdown-export SHA-256 values were, in the same order,
`aa559e964a6658d6e5e1d82ce6fb8768be0afed8a0a6cd9261eb3f174e6da657`,
`c89b5eb4511055948caeb5e44ab6e811151b033c0be7b09bdf0922c0e3e6cc9a`,
`93b093ae1501b341bd7f5f67a63d9f84958002de6de8a4c8827ef66bdeb7b9c5`,
and `09d87d43b85ac61cad71bef4c99d0af8397912170acb76bac2478a9d4fef59b7`.

The `mant` integration test `annotated_preview_four_pages_keep_one_surface_across_real_viewports`
loads each document once, checks the final row count, and renders the same
Fixed result into real Ratatui buffers of widths 20, 40, 78, and 120. Its
fast default mode samples an initial viewport. The separately executed
ignored audit `annotated_preview_full_body_matches_real_tui_buffer_at_four_widths`
compared every visible cell symbol at horizontal offset zero in bounded
128-row Ratatui buffers against the literal text consumer at each of those
four widths. It did not compare styling, link metadata, or off-screen columns.
All 16 page/width probes reported
`complete=true`: respectively 31,888, 1,651, 676 and 90,871 rows per width.
Only the viewport clips; no native rerender is performed at a new width.
`annotated_preview_loads_all_four_representative_pages_end_to_end` separately
exercises the real argument/load/query/presentation entry. The 1,000-row
allbox regression checks 2,000 distinct cell regions through the new codec.

## CVS comparison and honest coverage

Using the repository's source-bound framing helpers, GCC and Git content
comparisons were complete and found no body differences. Clang's automatic
frame recognized only a partial footer; after removing the observed fixed
CVS footer and the CLI title, all 676 body rows matched exactly. rclone's
automatic frame did not recognize its header/footer; after excluding those
observed reference rows and the CLI title, the entire body had identical
non-whitespace visible character sequence (2,286,549 characters) with no
missing or duplicated visible glyph. Its native rows differ: 90,889 in
pristine CVS versus 90,871 in the candidate. A confirmed sample follows
approved vendor patch `0013-pandoc-verbatim-fonts.patch`: pristine CVS rejects
`\\f[V]`, while the patched renderer accepts it; later `roff.c` hyphen
preprocessing and `term.c::term_fill` produce different wrapping. The small
`collector_preserves_patched_verbatim_font_wrap` test runs the exact source
through CVS first and verifies the native collector preserves the patched
renderer, without attributing every rclone row difference to that one rule.

An independent patched-raw differential closes the current collector and
consumer geometry question. The ignored `libmandoc-rs` audit test writes the
approved patched `Renderer::Utf8` bytes for these exact fixtures under
`target/annotated-g1-patched-raw`. Then
`python3 scripts/check-annotated-g1-patched-geometry.py` uses the repository's
conservative overstrike scanner, verifies each page's observed two-row
header/separator and separator/footer shapes, strips only those four furniture
rows, and compares the **entire normalized visible body**, including spaces
and row breaks, with the release annotated-preview CLI. The raw UTF-8 is
decoded and SGR/overstrike sequences are folded first; this is not raw
renderer-byte equality. All four reported `complete=true`, no retained C0
controls except newlines and zero differing rows:

| Page | Complete rows | Patched raw body = framed CLI body SHA-256 |
| --- | ---: | --- |
| GCC | 31,888 | `f0e7e37aa63ce0ef43d0ff8e3cf70c6736a7d92a449fe3ad219056b675407227` |
| Git | 1,651 | `4d553225a9caa5309751bb73cdcd835b2319f399923c644030c14dc8423714bd` |
| Clang | 676 | `4ce14a2e3d145902606534df753453634967e91d994dd499756f1a7dd08aa222` |
| rclone | 90,871 | `62c7cab4e4997f8e472e483ec4007c609fb7023b1b721c7451022c758b89aca3` |

The older pristine-CVS geometry comparator still reports **uncovered** on
these large pages; it is not relabeled as a pass. The exact patched-raw check
instead proves the new collector/IR/CLI did not invent or lose normalized
visible page-body whitespace or physical rows relative to the actual approved
vendor renderer. The raw-renderer test and CLI use virtual source names that
differ in their compression suffix; this geometry check does not prove equal
source identities, diagnostics, or include resolution.
It does **not** claim the pristine CVS and approved patched renderer have
identical rclone wraps; the 18-row difference remains tied to that distinct
comparison boundary, with one independently proven font-patch example.

## Cost and interactive checks

`/usr/bin/time` measured separate release CLI processes on the same fixtures
in two inverted operation orders (seconds, peak RSS KiB). These are observed
ranges, not stable benchmarks; P0 old-path numbers are in the linked record.

| Page | Text s / KiB | Outline s / KiB | Explain s / KiB | Visible search s / KiB |
| --- | --- | --- | --- | --- |
| GCC | .41–.42 / 91,364–91,564 | .39–.40 / 91,372–91,760 | .39–.41 / 91,568–91,940 | .45–.48 / 102,440–102,708 |
| Git | .01 / 10,912–11,000 | .01 / 11,792–12,012 | .01 / 11,236–11,748 | .02 / 14,460–14,848 |
| Clang | <.01 / 10,004–10,060 | <.01 / 10,824–10,888 | .01 / 10,508–10,888 | .01 / 11,716–12,100 |
| rclone | .82–.87 / 221,952–222,028 | .79–.90 / ≈222,168 | .80–.82 / 222,044–222,168 | .90–1.00 / 222,048–222,240 |

The new rclone elapsed time is about 1.5–1.7× P0 old-path release time,
while peak RSS is lower (about 217 MiB versus 239 MiB). GCC is around
1.3× on direct text at similar RSS. The extra annotated surface/mark
construction and new response validation are plausible costs, not an
isolated profile attribution. Two runs cannot establish a stable speed
ratio. Native-only annotated capture on the same four files measured about
324–332 ms, 12–13 ms, 6 ms, and 649–651 ms respectively; those are not
CLI timings and the earlier R01 capture had less annotation coverage.

The manually run release-stage test `annotated_preview_four_page_stage_costs`
repeated in opposite page order (milliseconds; native excludes decode, and
each later stage uses the same decoded Fixed result):

| Page | Decode | Native capture | Fixed IR lowering | Outline | Explain | Visible search | Text render |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| GCC | 5.0–5.8 | 304.6–334.3 | 38.1–44.9 | 8.8–10.5 | 9.8–9.9 | 58.8–77.2 | 6.6–6.7 |
| Git | .3 | 12.7–13.1 | .7–.8 | .3–.4 | .3 | 1.7–2.0 | .2–.3 |
| Clang | .1–.2 | 6.7–6.9 | .6–.7 | .2 | .2 | .7 | .1 |
| rclone | 5.6–5.7 | 635.3–666.3 | 96.8–98.3 | 33.5–33.8 | 34.7–35.3 | 172.6–175.2 | 15.4–16.4 |

The selected explanation terms returned zero verified semantic entries on
these four pages at P1; this is an honest R04/R05 semantics boundary, not
proof that the display or query process failed. Search returned 10/4/1/10
retained occurrences respectively. These stage figures are same-process
observations and do not include CLI startup or peak RSS. Stage search uses
case-sensitive matching with a 10-result limit, unlike the CLI's default
case-insensitive 100-result query; it is not a like-for-like CLI search cost
decomposition. The separate-process
table above provides that context.

On a real `xterm-256color` PTY, the reproducible
`python3 scripts/check-annotated-g1-pty.py` run opened Clang in both Fixed
pager and TUI, sent pager horizontal moves or TUI Shift+Right, resized both
from 70 to 20 to 120 columns, accepted `q`, and verified alternate-screen
and terminal-mode restoration (2,596 and 2,593 captured bytes on the final
rerun). Source-neutral
Fixed UI tests cover actual hit/reveal/selection-copy coordinates and the
Fixed node-copy menu now honestly directs users to exact visual selection.
The PTY probe does not assert a before/after screen diff for horizontal moves
or resize, nor automate link clicking or clipboard access; those behaviors
remain unit-test-backed rather than terminal-captured claims.

## Independent feature and source checks

The four `libmandoc-rs --no-default-features` test builds were run separately,
not inferred from workspace feature unification: parser-only 96 unit tests,
render-only 97, annotated-only 107, and render+annotated 108, with their
feature-relevant integration and doctests passing (manual audit tests remain
ignored by default). The approved 40-patch series replayed offline with
`crates/libmandoc-rs/scripts/sync-vendor --verify --archive
target/mandoc-migration/freeze-cvs-20260920T122115Z/upstream-cvs-20260920T122115Z.tar.gz`
and reported `vendor is up-to-date`. The old public structured API is still
present until R09, and its passing tests are not counted as annotated-path
evidence.

The local `scripts/check-packaged-crates.sh` run passed against freshly
extracted package sources, including independent codec `native-annotated`
and `mant` `annotated-preview` tests. Its disposable workspace used the same
checked-in fixture corpus without adding those files to crate archives.
Windows/macOS and CI-host verification have not been run.
