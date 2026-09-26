# EN00: annotated entry recognition baseline

This is the frozen, pre-change evidence for the EN00–EN06 work in the
workspace-adjacent `tmp/review/mant-annotated-entry-recognition-guide-2026-09-26.md`.
The guide lives outside this repository; this record and its inputs do not.
The roff inputs below are synthetic probes, not copied real manuals.

## Identity and method

- Branch/HEAD: `dev`, `918ae0a7001a710055cd841db03e1461f7cf01d8`;
  tracked worktree clean before this record was added.
- Build: `cargo build --locked --release -p mant --features annotated-preview`
  using the repository `target`; release `mant` SHA-256
  `f1a17b0e10b680b8883122b5d473b2ee101848363415b82fb161a3fec5585c6e`.
- Behavioral reference: `target/mandoc-migration/reference/mandoc`, SHA-256
  `d7c58752e2586695d17f4a87cc5540b35ec65ebec51bbcb5de8c9e6786e7311d`.
  All five exact inputs passed `-Tutf8 -Owidth=78` before any behavioral
  assertion was added. The reference establishes display/execution facts,
  **not** ManT entry kinds. Relevant pinned CVS paths include
  `man_term.c::pre_IP/pre_TP/pre_alternate/pre_B/pre_PP/pre_RS`,
  `mdoc_term.c::termp_it_pre`, `mdoc_macro.c::in_line`, and `term.c::term_word`.
- Existing Fixed query gold: `python3 scripts/annotated_fixed_query_gold.py
  --self-check` and `--cli target/release/mant` passed (51 queries); no
  expectations were rewritten to match the new work.
- Benchmarks: `python3 scripts/annotated_perf_baseline.py --frozen-dir
  target/entry-en00.oXEsSw --out target/entry-en00.oXEsSw/run-918ae0a7`.
  The raw `results.json` has SHA-256
  `b8d138130a13cb12e4c27107989244a1f4b4c1669cc2ea1113ab069d53f8d9a0`.
  The benchmark's frozen native binary has a *different* SHA from the
  behavioral reference and must not be substituted for it. `target` is
  disposable, so the hashes and runner command identify this local capture.

| Input | Source SHA-256 | CVS UTF-8 stdout SHA-256 on this host |
| --- | --- | --- |
| `inputs/environment.1` | `99460b653aa207f03f99361ce220343d47db3447a99dee2bf97b25d333bd065a` | `2bae2418e1528cb7bf6c6ed330c722393d65a81e8d2cf1c564d477ef84d84489` |
| `inputs/roles.1` | `006b2ca499f693930aa8d38b1380a82f6514a0b471550094faf0b81878e0067d` | `4bf72c733932fe51013b214a8d3296db60cdd1e8de1ecc29c44cad7296ac314e` |
| `inputs/catalog.1` | `82d997f7730fb9b03d1644b4fe78c367f30168a4dbe98ead1d69e92178fa5dda` | `dfd35e22e4852143026fff39195087e521f4dd7273fbcaba3f6af7781d3d65da` |
| `inputs/note.1` | `e399b0acffa55cbf33fcf073842066fe2fb8d8451bed40e3f9fa4d4bf068ddba` | `48ca26ba43cd966173987330f3bffb751fac56a5db993141668cc63293167767` |
| `inputs/terms.1` | `05bbb60f762225f5b6bd32345ffb1f4841851b509dc3253eb055766784925750` | `14f9f0fdb1541ce509ff6cd1132d98f2972fe68cadb4d9e82b2355f2d157d6b0` |

E03b also passed `-Thtml -Ofragment` (781 bytes, stdout SHA-256
`fd647a8e60d586271eafb5443bddbefd92a4fcf22683f000c0786d3279c15a7a`).
The mdoc footer can depend on the running host, so these output hashes are
local evidence, not portable golden bytes.

## Semantic observations, not gold copied from Flow

Both routes were run with `--input-format roff --outline --outline-entries
all --outline-references none --format json --compact`; the Fixed route also
used `--annotated-preview`. The `--explain=NAME --display direct` JSON was
checked for each name below. Counts are direct-definition counts, not a claim
that all other evidence categories were absent.

| Input | Baseline Flow | Baseline Fixed | Reviewed ManT contract |
| --- | --- | --- | --- |
| E01 environment | CPATH, TMPDIR, TEMP, TMP, lower_name: 1 each | CPATH/TMPDIR/TEMP/TMP: 0; lower_name: 1, but Term | Three owners: CPATH Env, TMPDIR/TEMP/TMP one Env owner with three names and no inferred aliases, lower_name Env; bullet and prose are not entries. |
| E02 roles | All nine queried names directly bound; counter/activity-action/MODE_FAST are Term, BatchMode ConfigKey | TEMP/TMP: 0; counter/MODE_FAST have empty-name Term; activity-action and BatchMode are Command | Ev Env, Va Variable, Ic without command context named Term, Dv named Term, Cm in configuration ConfigKey, Ic in commands Command; three Ev names on one owner. |
| E03 catalog | git-add: 1, Important: 0 | git-add: 0, Important: 0 | Only the COMMANDS PP/RS `git-add(1)` becomes a direct Command; SEE ALSO remains reference/mention; introduction stays prose. |
| E03b note | Note: 1, git-add: 1 | Note: 0, git-add: 0 | A bare bold `Note` is not a weak PP/RS declaration; complete `git-add(1)` is. The two have the same native paragraph/indent structure, so this is a ManT syntax policy, not a mandoc type claim. |
| E04 terms | fast, full diagnostic phrase, working tree, template: 1 each | Same direct results; template is named Term | `fast` and the full diagnostic/multiword labels remain named Term, not guessed Values or split names. The template keeps a readable owner but has no semantic entry; its HEAD-only mention must remain discoverable. |

Fixed currently reports `semanticsComplete=false` for these probes and emits
coverage diagnostics. This is an observed state, not a reason to count a
missing direct definition as passed. The E01 Fixed bullet currently appears
as an empty-name Term in outline. E03b Flow currently makes `Note` a false
direct Command. The guide's semantic judgments above were made before the
implementation, while the exact native display was checked against CVS.

## Real-page review panel

Twelve existing, repository-owned real fixtures from five source families
were each rendered completely with the same pinned CVS UTF-8 command (12/12
exit 0). The 23 listed queries were checked with the frozen release Fixed
route. The source SHA-256 identifies **decompressed roff**, not the compressed
container; fixture acquisition and license records remain in
`tests/fixtures/roff/real/README.md`. This is a small manually reviewed panel,
not a claim that the historical 120-page/199-query archive has been rerun or
that all possible names on these pages are correct.

| Repository fixture under `tests/fixtures/roff/real/` | Decoded roff SHA-256 | Reviewed query: baseline Fixed direct count → EN00 judgment |
| --- | --- | --- |
| `archlinux/gcc.1.gz` | `a86bd1d671aa2afff558eb3b7c8b6e2a8bbaf55cb9fb2a6549b7538dd56e0f0f` | `CPATH`: 0 → Env direct 1, independent `.IP`; `-x`: 2 → retain two independent Option owners, not one merged body. |
| `archlinux/gzip.1.gz` | `107c35463c2fb970318b34ccca09db100e1b99bfab793196eab68ef91d350b78` | `file: not in gzip format`: 1 → retain one *complete* named Term, not `file`; `--ascii`: 1 → retain Option. |
| `archlinux/zip.1.gz` | `b8cd4f0980a6a3abd243c00a146bf336564b2bf6c270d1101f541da4d9796623` | `-A`: 1 → retain Option; `zipcloak`: 0 → introductory `.PP .B` prose, no direct entry on this evidence. |
| `archlinux/zsh.1.gz` | `fee817f32be2ca893147affc5b9f82dce17ad8f44d77fcb40839450ea94dd15d` | `-c`: 1 → retain Option. |
| `archlinux/yay.8` | `7f5115d15c9647b77bbc4aa838dff90818a20f03763f1557e1a0e36292ab614e` | `--yay`: 1 → retain Option from complete `-Y,--yay` declaration; do not infer an alias group from punctuation alone. |
| `fedora44/git.1.zst` | `2ee1c5dd84a69dfc91d84840e943e7df0ee06b86bf27eb4bb9369c415f3351c1` | `git-add`: 0 → Command direct 1 from COMMANDS `git-add(1)` plus direct `.RS`; `GIT_ADVICE`: 0 → **do not hard-gold direct 1 yet**: its ENVIRONMENT PP/RS may lack the independent weak-declaration evidence required by §5.3; adjudicate in EN06. |
| `fedora44/clang.1.zst` | `8f727b7a3966a90989f474259bab124fdb3935913dffcb6912c1237a63b2b241` | `TMPDIR`, `TEMP`, `TMP`: 0 each → one Env owner with three separately bound names, no inferred aliases. |
| `debian/sh.1.gz` | `0f6252ae51279e02e6b6f461f5ab17099f477ccc2f0086d3ab455f837356a1a1` | `HOME`: 1 and `CDPATH`: 1 → retain separate Ev-backed Env owners. |
| `bsd-closure/netbsd-drm.4` | `de68d037313c80fce7ecdb72da76e09e6009aafba7ad6673d1a6e7df7c6ed9a3` | `INSECURE`: 0 → prose `.Dv` mention only; `amdgpu`: 1 with `names=[]` → independent named Term if the full `.It` head binds. |
| `bsd-closure/dragonfly-adduser.8.gz` | `d2c2c09556f74e93bac34e9e3d17032c4f66daea96367854a096fbcc062d733e` | `randompass`: 0 → body `.Va` mention only; `random`: 1 Command → named Term, or Value only after checked local `Fl w Ar type` parent/group proof. |
| `windows-releases/rg.1.zst` | `e83d82c28bb2683cf71500e5fb8d9746a162bb24b08a6396a15ab2ed1d96cb45` | `--glob`: 1 and `--threads`: 1 → retain Option direct definitions. |
| `windows-releases/rclone.1.zst` | `f35de3b3008f684a7db141a7626db68b08c46e5b3d45c97726bc6d97eebade4e` | `--help`: 0 → example/prose mention, not direct; `--transfers`: 0 → `.SS` heading-adapter positive candidate explicitly deferred to EN05. |

The BSD-closure fixtures include Va and Dv **body references**, not an Ev
definition-head example. E02 supplies a synthetic Ev/Va/Dv positive matrix;
we must not call this real panel a positive real-BSD role test. A new real
fixture would require the usual source/license and all audit-ledger records.
All 23 current Fixed query responses reported incomplete semantics, so
classification improvements must be assessed together with coverage rather
than recasting an observed omission as `clean`.

## Pre-change cost reference

The runner used 12 interleaved output trials and 8 query trials per page,
fresh process and one warm-up per command. These are median output-route
wall time / peak RSS; full quartiles, CPU, command lines, query measurements,
stdout hashes and machine identity are in the raw report. `old` is the
existing Flow route; `annotated` is the preview route. Different routes need
not have byte-identical consumer output; compare each route to its own
pre-change hash and compare native *content* under the documented fidelity
normalization.

| Page | Native | Flow | Annotated |
| --- | --- | --- | --- |
| Clang | 3.58 ms / 3,448 KiB | 8.89 ms / 13,120 KiB | 9.81 ms / 10,584 KiB |
| GCC | 73.11 ms / 18,296 KiB | 301.54 ms / 90,138 KiB | 384.64 ms / 58,604 KiB |
| Git | 4.42 ms / 3,820 KiB | 14.08 ms / 14,914 KiB | 17.09 ms / 11,734 KiB |
| rclone | 150.13 ms / 54,038 KiB | 535.43 ms / 244,228 KiB | 738.17 ms / 144,454 KiB |

This baseline does not claim that EN01–EN06 has passed its gate. Semantic
recognition may add entry and query work; later reports must separate that
cost from a pure performance regression. The optional heading adapter and
cross-page value-domain inference are deliberately deferred.
