# Roff semantic-entry precision audit

`SEMANTIC_AUDIT.csv` records an independent audit of semantic entries created
after native roff lowering. Target conservation proves that zero-width
destinations survive; it cannot prove that numbered prose, placement-only
definitions, or invalid value-domain children were not promoted into the
agent-visible semantic index.

## Oracle and scope

The development-only `roff_semantic_profile` example parses and lowers each
page once, builds the final `SemanticIndex`, and records each entry's ID, kind,
aliases, visible forms, targets, containing section, nested depth, and
value-domain origin. Profile schema `mant.roff-semantic-profile/v1` also walks
the IR definition lists independently so an ordinal that failed to become a
list cannot hide merely because semantic discovery declined it.

The following are high-confidence review findings:

- a punctuated integer definition such as `1.`, `2)`, `(3)`, or `[4]` remains
  in a definition list;
- such a form becomes a `term` or `value` semantic entry;
- an entry has neither a semantic alias nor any visible form; or
- a `Choices` value domain contains a child whose kind is not `value`.

The profile separately counts aliasless generic terms and entries below
NOTES, FOOTNOTES, or REFERENCES headings. Those are sampling signals, not
automatic failures: real manuals can intentionally define terms in those
sections, and a source-neutral audit must not turn an English heading guess
into a lowering rule.

Rows use `(corpus, path, decompressed-source SHA-256)` identity. A `review` or
`hard-failure` starts as `pending` and requires a durable `false-positive`,
`confirmed-open`, or `confirmed-fixed` conclusion with a useful note. Confirmed
product defects become licensed real fixtures and focused Rust tests. The JSON
report additionally records the producer commit, profiler binary SHA-256,
timestamp, corpus roots, page count, entry count, and kind totals so a broad
scan cannot be cited independently of the code that produced it.

## Reproducible fixture gate

The checked-in corpus is part of the Unix verification boundary:

```sh
cargo build --locked -p mant-engine --example roff_semantic_profile
python3 scripts/audit-roff-semantics.py --fixtures --recheck-recorded \
  --verify --findings-only
```

The current fixture inventory contains 37 clean pages and 10,727 semantic
entries. It has no punctuated ordinal definitions or entries, empty entries,
or value-domain violations. Aliasless generic and note-like counts remain
visible in the command summary for deliberate sampling.

## Distribution sweep

Run a complete local hierarchy as release-time evidence rather than making
host manuals a CI dependency:

```sh
python3 scripts/audit-roff-semantics.py --manpath /usr/share/man \
  --corpus archlinux-host --recheck-recorded --findings-only \
  --json /tmp/mant-semantic-archlinux-v1.json
```

Keep the JSON report under `/tmp`; it can contain host-specific paths and
bounded entry samples. Record the exact producer commit, profiler hash, corpus
identity, page/entry totals, and reviewed findings here when a complete sweep
is accepted. A clean automated scan is evidence for the checks above, not a
general proof that every source's semantic classification is ideal.

On 2026-09-04 the complete local Arch Linux hierarchy was scanned at producer
commit `c51be9d8a539080dd8f53e8812386d8c7b4f782e`. The profiler binary SHA-256
was `2d81071c571963bbbef607908665b3392febb7f037aa5715b1bbb09412b6e80e`.
All 28,712 pages were clean. Their 347,943 semantic entries contained no
punctuated ordinal entries, retained ordinal definitions, empty entries, or
value-domain violations. The informational census found 241,323 aliasless
generic terms and 6,914 entries in note-like sections; these remain sampling
populations rather than automated defects. The exact JSON report is
`/tmp/mant-semantic-archlinux-v1-c51be9d.json` on the audit host.

The broader 45,036-path restored logical inventory described in
[TARGET_AUDIT.md](TARGET_AUDIT.md) was rescanned on 2026-09-04 at producer
commit `9233968b21f046bdcf301f5b34954ee43cdd2292`. The v1 semantic profiler
binary SHA-256 was
`597b0db7de4380bafe1e3d39ab1aed219da59f9274e241828c607c6eb52d5b52`.
All 45,036 pages were clean. Their 651,428 semantic entries contained zero
punctuated ordinal entries, retained ordinal definitions, empty entries, or
value-domain violations. The informational census contained 439,875
aliasless generic terms and 10,063 entries in note-like sections; these remain
review populations, not automatic defects. The exact reports on the audit host
are `/tmp/mant-semantic-full-45036-9233968.json` and
`/tmp/mant-semantic-full-45036-9233968.csv`.
