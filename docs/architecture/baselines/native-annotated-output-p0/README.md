# P0 annotated-output baseline at `864aa042`

This is a frozen *old production-path* comparison, not an annotated-path
acceptance. Branch `dev` was clean at `864aa042d859bd793cfd74af90e9bc4cf3571cb4`
when captured on 2026-09-23. The previous implementation commit is
`fa3e9869`; the private structured path still rejects the four representative
pages. All builds and audit artifacts used this repository's `target`.

The fixed oracle preflight passed with identity
`cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1`, registered attestation
`crates/libmandoc-rs/upstream/oracle/attestations/cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1.json`
(SHA-256 `72aca4b181b0463d41ae24138d1f62fdf19cebb9e543af260fbbc70f75d4bde6`),
pristine archive SHA-256
`6ccee6e73346b3e2c9701ba70dc5ac0525f2dd160cfe98ffc00ad56371d28c7d`,
and `target/mandoc-migration/reference/mandoc` SHA-256
`d7c58752e2586695d17f4a87cc5540b35ec65ebec51bbcb5de8c9e6786e7311d`.
The 36-patch vendor series was inspected, not replayed in this R00 capture.
The exact preflight command is:

```sh
scripts/mandoc-oracle-preflight \
  --attestation crates/libmandoc-rs/upstream/oracle/attestations/cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1.json \
  --binary target/mandoc-migration/reference/mandoc \
  --archive target/mandoc-migration/freeze-cvs-20260920T122115Z/upstream-cvs-20260920T122115Z.tar.gz \
  --identity cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1 --profile utf8
```

## Inputs and old output fingerprints

Paths below are relative to `tests/fixtures/roff/real/`. Input hashes are
*decoded roff bytes*. `CVS` hashes are pristine `-Tutf8 -O width=78` stdout;
`text` hashes are `mant --format text --display direct --color never`
stdout, including application presentation. These hashes are not expected to
match each other without role-aware framing. JSON hashes use `--compact`.

| Page | Input path | Decoded bytes / SHA-256 | CVS raw SHA-256 | Old text SHA-256 |
| --- | --- | --- | --- | --- |
| GCC | `archlinux/gcc.1.gz` | 1,621,846 / `a86bd1d671aa2afff558eb3b7c8b6e2a8bbaf55cb9fb2a6549b7538dd56e0f0f` | `4eb0dd1673e918859af1276a7d95603bc1c7657b204668990604470b2987b518` | `8e73ba5873a8efadc3bfda130ba4abbd0eb2b2912318f55ec633b7ea64f5fca7` |
| Git | `archlinux/git.1.gz` | 67,376 / `2736b9d20cd9a36c7971ccdbd7ba5e71cea543960340bb9a27984369df559862` | `3bbbe70ff768f09edb7f67e68b3c421843e333e0d2813133ffaccc40ad72d47f` | `5754accff6a3d36d0f78a2aa01e93faf0b56b9624d86559290fbfba271d7a99f` |
| Clang | `archlinux/clang.1.gz` | 28,479 / `ff10a1611fc293291388feabf2d988c2c8950c0e784a34d4fcea5b4eb966fce0` | `c749ab8e47218cd2a04c115582d65d662f60da29d0cdb7cac51edf4705a916ed` | `321d97119bf0ce673fe612c57d981231ead66a601e093c0f94dc1420a2487812` |
| rclone | `windows-releases/rclone.1.zst` | 3,346,767 / `f35de3b3008f684a7db141a7626db68b08c46e5b3d45c97726bc6d97eebade4e` | `f0dfbd486753167ee349795ca37c67c78f84a4ff4288c343e19c3613f809df97` | `cd5663dd2cd17805a37c9f4bbf156fdfb553200431ae3afff745204f671c0014` |

| Page | Old document JSON | Old outline JSON | Old explain JSON / request |
| --- | --- | --- | --- |
| GCC | `12c9c32a1093fa0c5cca8e6e7a4133eb59b0b74e4b671315cc113161ca895a7c` | `db6de1ea1776004c8ef721af1acea61d6036b981093edfbf0e74452076682bfc` | `5c741ac8354f24f33ede8d735ba4cc20c724acdf0b5b8bfabd335e3c58549a6d` / `-x` |
| Git | `2c9314f51221b17e39070178c02e7ad005eeb1a57c9c48bba167ac131c073d6f` | `a49240fcf81b02b49209a0ace512c4d566fdbf9a3f84d4a5b513dde74d7d5f79` | `4e4e79c516b439578dba0d89ea8777af98a7ba5f30699ee07fca1ec05f7d6d8f` / `--help` |
| Clang | `dbead60af0fa599945c39da9b5867689200602d6994dd773b0dfcd00461` | `3de6e7a93a5694f4315fde345bfff71124591fe2b2391194dd095722945f2f48` | `fe342bbd111431aa02bfbb03a0e8484815af72aa8230009ef4aa879046e4d0c4` / `-help` |
| rclone | `b36e64f1f575b2ad1f0254f4d521f90f3e08831d4c0d25568e33968b406685fe` | `a2498e4a1750ebd9da5bc4ccedc106c91e4bf5ebb62691bf7acfb7dd0398a736` | `e8a4cc3587c693d12b4e2b1a5defd34de5d2ac81c6469c82fe30f84f404e82d2` / `--help` |

The same text/document/outline/explain SHA-256 values were independently
observed from freshly built debug and release CLI binaries. Explicit visible
search used `--search=QUERY --limit 10 --format json --compact`:

| Page / query | Old visible-search JSON SHA-256 (both builds) |
| --- | --- |
| GCC / `-x` | `c2eb186df65bf1af7fdbbcb821b896f8d60ee000d3a2f240d693d20243deaf59` |
| Git / `--help` | `7e2d71b199d12b1cc5c540b81fccb592a6de1088185415a2a0ea341eacec976b` |
| Clang / `-help` | `2bc4f91b7151aea06ef3aad46c71166c8c8c74853a56ab13c15efb77e4d42423` |
| rclone / `--help` | `b95645f55956a25326bee33bf2951c10fcea35d1daf3ec5b7fa80d7b89df9e65` |

This is the old Markdown-coordinate, line-group search contract; the planned
occurrence protocol will intentionally change these JSON fingerprints.
The GCC `-x` and rclone `--help` topics have existing independent query gold.
Git and Clang explain output above is only a captured baseline, not newly
reviewed semantic expected data. The old `native-structured-rendering-c05`
record's private `Unsupported` results remain a historical fact and are not
mistaken for successful annotated output.

## Measured old-path cost

`cargo build --locked --release -p mant` and
`cargo build --locked --release -p mant-engine --example measure_native_load`
both passed at this baseline; debug builds of the same targets passed. The
release CLI SHA-256 was
`6b44096af422593a1d37b802ad935aa9f06f0e912d6e380805745ef148e635cb`;
debug CLI SHA-256 was
`99098b58ab1cf5358afd32548636e262c8b85edd5a5b8089a40d9eae8a2bcccc`.
The release example SHA-256 was
`8a085e898136e656c8db1c532e43dd1bd4fd0e349631ddf2e19a203e4c4161e7`.

`/usr/bin/time` measured one separate process per page/operation, including
CLI startup, decompression, load and projection. Values below are single
observations, not performance thresholds or statistically stable medians:

| Page | release text / outline / explain / search elapsed s | release peak RSS KiB (text / outline / explain / search) | debug text / outline / explain / search elapsed s | debug peak RSS KiB (text / outline / explain / search) |
| --- | --- | --- | --- | --- |
| GCC | 0.31 / 0.30 / 0.33 / 0.40 | 89,712 / 89,628 / 89,816 / 90,844 | 1.53 / 1.53 / 1.68 / 2.02 | 104,364 / 104,996 / 104,864 / 107,120 |
| Git | 0.01 / 0.01 / 0.01 / 0.01 | 13,784 / 14,168 / 14,360 / 15,184 | 0.07 / 0.07 / 0.08 / 0.09 | 29,032 / 30,968 / 32,120 / 32,776 |
| Clang | <0.01 / <0.01 / <0.01 / 0.01 | 11,968 / 12,420 / 12,244 / 13,240 | 0.03 / 0.03 / 0.03 / 0.04 | 26,924 / 29,028 / 29,940 / 31,600 |
| rclone | 0.53 / 0.48 / 0.58 / 0.63 | 244,368 / 244,032 / 244,316 / 245,476 | 2.19 / 2.03 / 2.55 / 2.90 | 258,636 / 259,500 / 259,032 / 262,080 |

The existing `measure_native_load` example accepts gzip only, decompresses
before timing and performs seven same-process samples. Its release load
milliseconds were GCC `276.7,247.3,242.9,238.1,238.2,239.0,242.7`, Git
`8.95,9.40,9.09,8.97,8.93,9.24,8.74`, and Clang
`5.18,5.05,4.99,5.29,5.03,4.97,5.09`. Debug load milliseconds were GCC
`1356,1355,1341,1334,1343,1338,1332`, Git `57.6,59.1,56.9,57.1,55.9,58.1,58.2`,
Clang `26.2,24.8,24.4,25.1,24.7,24.9,24.7`.
Release post-load samples (milliseconds) included GCC index `5.3–7.2`,
outline `2.5–4.2`, explain `52.8–57.0`; Git index `0.17–0.22`, outline
`0.086–0.146`, explain `1.16–1.31`; Clang index `0.047–0.081`, outline
`0.017–0.057`, explain `0.405–0.487`. rclone needs a separate zstd-capable
same-process harness if stage timing rather than total CLI time is required.

## Reproduction and comparison scope

Run from the repository root. Set `PATH` to
`tests/fixtures/roff/real/` plus the path in the input table; for example,
GCC is `tests/fixtures/roff/real/archlinux/gcc.1.gz`. The compact JSON hashes
above were measured with exactly that repository-root-relative spelling,
which can affect source identity. `QUERY` is the value in the explain/search
tables. `PROFILE` is `release` or `debug`:

```sh
target/PROFILE/mant --input PATH --format text --display direct --color never
target/PROFILE/mant --input PATH --format json --compact
target/PROFILE/mant --input PATH --outline --outline-entries all --format json --compact
target/PROFILE/mant --input PATH --explain=QUERY --format json --compact
target/PROFILE/mant --input PATH --search=QUERY --limit 10 --format json --compact
target/PROFILE/examples/measure_native_load PATH load QUERY # gzip only
```

`PROFILE` and `PATH` in this shell block are explanatory placeholders, not
shell variables to expand literally. One exact small command is
`target/release/mant --input tests/fixtures/roff/real/archlinux/git.1.gz
--search=--help --limit 10 --format json --compact`. To regenerate after later
source changes, use a detached checkout of commit `864aa042` and set
`CARGO_TARGET_DIR=/home/hby/dev/ManT/target` for all Cargo builds; do not
compile in a second target tree. The source commit and binary/output hashes
above identify the old state. The 78-column raw CVS and old CLI text bytes
for all four pages remain in the local audit directory described below;
larger old JSON responses are fingerprinted here rather than duplicated as
multi-megabyte tracked artifacts.

The source-bound 78-column differential audit ran with the checked-in script
`scripts/audit-roff-rendering.py`, registered oracle and four-page manifest.
Its raw artifacts are retained under `target/annotated-p0-baseline/release-audit-78/`
on this host; `summary.json` SHA-256 is
`f5a1944eedc0a0cccb4267c9677e3f3579152d5961635e3cce5aef03f76dc028`.
It classified one page `partial` and three `review`, not four clean matches.
Its `coverageComplete` was `false` and **all four geometry comparisons were
`uncovered`**; one content comparison was covered and three required review.
Those are old-path results and should not be used as the new route's expected
text. `scripts/audit-roff-rendering.py --width 78` controls the *oracle's*
native width; the old CLI has its own width path. G1 must compare a single
78-column Fixed result at several UI viewport widths, not call the native
renderer four times to simulate resize. `geometry_audit` is bounded by
20,000 rows/1,000,000 cells; `complete=false` is incomplete evidence.

R01 remeasures native capture; G1 remeasures full IR plus true CLI/TUI; R08
and R10 are later separate baselines. Compare on the same input hashes,
release/debug mode, output/query operation, cache state and decompression
policy. Alternate runs and report ranges before claiming a speed change.
No current figure proves the new path is faster or slower.
