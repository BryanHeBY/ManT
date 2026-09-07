# Classified explanation verification

Verified on 2026-09-07 (UTC), Linux x86_64. The implementation baseline is
`752b8539`; its preceding focused commits are `0dac0968` (literal punctuation),
`1d03acc3` (semantic export policy), and `a76b20c0` (protocol classification
types). This records the semantic-entry shell/explain review's E01/E02 and
classified-query acceptance, not the separate IR-convergence guide.

## Independent regression oracles

- `explanation_literals`: executable punctuation, finite sentence boundaries,
  exact case, Unicode and transparent inline/code wrappers. A short `-#` or
  `--` no longer obtains support from `-###` or `--%`.
- `semantic_export_attached`: fixed and inferred attached-value policies,
  visible placeholders, alias groups/relations, nested choices and whole-
  document ordinary-Markdown fallback when no common policy preserves facts.
- `explanation_classes`: independent owners, merged match bases, real generic
  owners without names, invalid name bindings, one-byte copies, off-page direct
  matches, and late direct/related entries displacing early mentions at 10,000.
- `explanation_previews`: two distinct original blocks, complete 512-scalar
  queries inside 1024-scalar windows, exact source/range/path resolution,
  nested item/table coordinates, controls, Markdown escaping and atomic bodies.
- `explanation::scoped`: class priority before BFS, same IDs in separate sources,
  continuous cross-class paging, summed global/local counts and source indices
  referring to readable reports rather than the loading graph.
- Protocol golden/closed-shape tests and `explanation_process`: required class,
  counts/order/previews, no old nested scope bodies, direct CLI/request-JSON
  equivalence, normal no-evidence/empty pages, partial sources and shared budget.
- Actual `mcp_stdio`: source-qualified original-read hints and concatenation of
  all 233-scalar pages back to the exact complete classified response, without
  changing semantic arguments or reordering within a character page.
- Existing original-content, real GCC/Bash, native lowering, choices coverage,
  alias metadata, strict navigation and semantic-export regressions stay active.

## Commands and results

```sh
cargo test --locked --workspace --all-features
cargo test --locked -p mant --test mcp_stdio
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo run --locked -p mant --example generate_help_tldr
bash scripts/update-protocol-schema-snapshot.sh
bash scripts/check.sh
```

The full-feature workspace passed 1,309 tests across 59 suites; six existing
tests remain ignored. The initial sandboxed attempt hit four loopback HTTP
bind denials in source-update tests; the complete run passed with local-port
permission. These were environment failures, not weakened test assertions.
The extended MCP character concatenation check also passed independently.

Strict Clippy and the complete `scripts/check.sh` gate passed. The gate covered
formatting, script/audit self-checks, workspace and profiler tests, optional
native renderer tests, symbol namespace, independently packaged crate tests,
read-only feature configuration, strict rustdoc, fuzz-target compilation,
optimized product build and executable smoke tests. The generated help TLDR
remained unchanged; only the authorized unreleased v0.11 schema was updated.

All 37 licensed fixtures were rechecked without rewriting their audit ledgers:

| Route | Result |
| --- | --- |
| CommonMark projection | 37 clean, 106 excerpts, zero review/hard findings |
| Target conservation | 37 clean, 14,985 classified owners/obligations; zero missing, unexpected, collision, invalid, duplicate or dangling targets |
| Semantic precision | 37 clean, 10,725 entries; zero ordinal, empty-entry, domain or conversion violations |

Fidelity/layout/structure audit self-checks and recorded coverage verification
also passed. These do not represent a new whole-host differential scan.

The optimized executable SHA-256 was
`f5ae1b25e993f0385de7941d944990825d8a3db4b0afa607d7c21c2fa7a4dbd4`.
Transient detailed logs use `/tmp/mant-classified-*`; this checked-in summary
and the named deterministic tests do not depend on those files surviving.

## Installed GCC spot check

```sh
target/debug/mant gcc --manual --explain=--help --format json --compact --display direct
target/debug/mant gcc --manual --explain=-# --format json --compact --display direct
gzip -cd /usr/share/man/man1/gcc.1.gz | sha256sum
```

The source hash is
`f31a9ae03e8e20baad471d47c4e17391038d852e32719c101001da6eb52b3083`.
The help query retained nine independent owners: two direct entries, zero
explicitly related entries, six entry mentions and one ordinary mention, with
no truncation. The aliasless Overall Options term remained an entry mention;
`-Q` remained a literal-only entry mention with a real match window. The short
`-#` query returned zero collected evidence. These counts describe this exact
installed page, not every GCC version.

## Limits

No native Windows/MSVC or macOS gate, new 45,036-page host sweep, TUI visual
session, sanitizer run or release operation was performed. Original IR/full
rendering is guarded by existing deterministic tests and fixture audits;
classification counts and clean validation are not exhaustive recall proofs.
No push, tag, main synchronization or source update was performed.
