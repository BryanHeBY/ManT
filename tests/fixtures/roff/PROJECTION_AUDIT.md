# Roff CommonMark projection audit ledger

`PROJECTION_AUDIT.csv` is the incremental, human-reviewed ledger for the
development-only IR-to-CommonMark topology audit. It is independent from both
the AST-to-IR structure ledger and reference-renderer comparisons: a page can
lower correctly and still be serialized as valid CommonMark with the wrong
block ownership.

The profiler renders each native document through ManT's public Markdown
projection, reparses that text with ManT's CommonMark parser, and compares:

- the exact section path, depth, title, and order;
- ordered-versus-bullet item order and nesting depth (adjacent CommonMark list
  containers may legally merge);
- fenced-block order, language, section, and owning-list nesting depth; and
- source HTML entity spellings in their original order, so CommonMark entity
  decoding cannot hide inside otherwise-correct topology;
- the first, middle, and last addressable section through the same excerpt
  renderer used by `--node`.

The current profiler schema is `mant.roff-projection-profile/v3`. v2 added the
entity-spelling oracle to the original topology checks. v3 treats an empty
roff section heading as a transparent formatter wrapper: CommonMark has no
addressable empty heading, but blocks and non-empty descendants below it remain
part of the topology comparison.

Tables and preformatted input both intentionally project to ordinary fenced
blocks. Display equations project to `math` fences. The audit therefore checks
the portable projection contract rather than expecting the reparsed Markdown
to reconstruct roff-only types.

| Field | Meaning |
| --- | --- |
| `corpus`, `path`, `section`, `source_sha256` | Exact decompressed-source identity shared by the other roff ledgers |
| `profile_schema` | Projection checks that produced the row |
| `scan_status` | Latest automatic result: `clean`, `review`, or `hard-failure` |
| `review_status` | Human disposition: `not-required`, `pending`, `false-positive`, `confirmed-open`, or `confirmed-fixed` |
| `note` | Reason for the disposition and any focused regression |

`clean` certifies only the structural and entity-spelling projection described
above. It does not certify terminal wrapping, typography, or reference-renderer parity. A
`review` row must be inspected in generated Markdown before it becomes a bug.

Build the profiler and scan the reproducible fixtures:

```sh
cargo build -p mant-engine --example roff_projection_profile
python3 scripts/audit-roff-projection.py --fixtures \
  --json /tmp/mant-projection.json
```

The ordinary command updates the incremental review ledger and leaves review
candidates for human disposition. The Unix verification boundary instead runs
the complete checked-in corpus as a read-only gate:

```sh
python3 scripts/audit-roff-projection.py --fixtures --recheck-recorded \
  --verify --findings-only
```

`--verify` never rewrites `PROJECTION_AUDIT.csv` and exits nonzero for either a
projection candidate or a hard profiler failure. This is the CI contract that
keeps every checked-in roff page behind the same CommonMark topology and entity
oracle.

Replay the exact unchanged inputs already recorded for a local corpus:

```sh
python3 scripts/audit-roff-projection.py --manpath /usr/share/man \
  --corpus archlinux-host --replay-fidelity-records --findings-only
```

`--recorded-only` rechecks the projection ledger; `--source-pattern` selects
high-risk syntax without sampling. A profiler schema change makes unchanged
rows eligible for a deliberate recheck. Host corpora never gate ordinary CI;
only the bounded checked-in fixture catalogue does. Larger local corpora remain
incremental review inputs, with focused Rust regressions derived from confirmed
findings.
