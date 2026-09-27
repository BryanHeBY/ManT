# EN06 real-page query review and completion record

This records the manually reviewed EN06 Fixed-query decisions and the
verification completed so far. The pre-change panel, baseline measurements,
and earlier query judgments remain in
[`../annotated-entry-en00/README.md`](../annotated-entry-en00/README.md).
The current candidate debug CLI passed the complete 79-query Fixed gold, but
that is not yet a clean, committed final-candidate gate or a performance result.

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

## Candidate verification and remaining gates

- Completed on the current candidate: the debug annotated-preview CLI passed
  all 79 source-bound Fixed gold queries. The tmux `activity-action` and
  adduser `random` named-Term targets are included in that result. The full
  four-page Fixed TUI buffer comparison passed at widths 20, 40, 78 and 120.
  The release G1 stage probe completed; its measurements and exact build
  identity still need to be recorded in the final candidate report.
- Still required for final attribution: rerun gold on the final committed
  candidate and record HEAD, CLI SHA-256, command and result. Complete the
  workspace, feature, packaged-source, schema and strict-lint gates, recording
  any environment blocker separately from a product pass.
- Still required for cost and display claims: freeze the final release binary
  and the same four decoded EN00 inputs, run the interleaved output/query
  benchmark, compare each route with its own EN00 identity, and explain any
  material change alongside the added entry work. Record G1 stage values,
  recognition/validation call counts and a scale probe. The existing process
  runner measures wall time, CPU and peak RSS, not allocation counts; no
  allocation measurement is claimed here. Existing default-route roff audit
  ledgers and earlier `target/entry-en06` output are not a final Fixed-route
  fidelity run.

EN05's declaration-heading adapter and unsupported cross-page value-domain
inference remain deferred. No new real fixture was imported in this unit, so
the existing source/license and coverage ledgers retain their prior scope.
