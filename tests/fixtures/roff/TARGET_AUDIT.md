# Roff target-conservation audit

`TARGET_AUDIT.csv` records the independent zero-width navigation audit. Visible
fidelity, AST topology, CommonMark projection, and renderer layout can all look
correct after an anchor disappears, so none of those ledgers substitutes for
this one.

## Oracle and scope

The development-only `roff_target_profile` example parses each page once and
lowers that exact owned `libmandoc-rs` report. Sharing one parser session is
part of the oracle: reparsing through a resolver with a different `.so` policy
can manufacture false losses on aggregate pages. Validated libmandoc
deep-link owners form the native evidence. Canonical section and entry IDs,
inline anchors, and exact source fragment aliases form the observed set.

Profile schema `mant.roff-target-profile/v3` classifies every deep-link owner
as `retained`, `excluded`, or `unclassified`, with a stable reason. Known man
definitions, mdoc lists/displays/functions, and inline semantic macros are
retained obligations. An owner macro without a policy is not silently omitted:
it is emitted in `unclassifiedOwners` and makes the page a review candidate.
Head/body/tail wrapper nodes at the same AST child-index path form one logical
obligation; independent same-named owners remain separate occurrences.

Every explicit `.Tg` spelling must resolve exactly. A spelling already equal
to its canonical identity needs no redundant alias; a noncanonical spelling
must survive as a fragment alias resolving to the normalized internal ID.
Generated destinations may use
ManT's deterministic canonical numeric collision suffix (`base`, `base-2`,
`base-3`, and so on). Each obligation records its AST path, source line, owner
macro and kind, section source line, expected IR role, and container class.
Observed section identities, entry identities, anchors, and fragment aliases
remain distinct occurrences and are consumed at most once. Consequently an
unrelated same-named section/entry, one retained occurrence standing in for two
owners, or an anchor in an incompatible structural container cannot manufacture
a clean result. The profile reports true unexpected targets, incompatible
identity-role collisions, invalid or empty identities/fragments, duplicate
identities, and dangling links separately. Semantic discovery can create
additional entry IDs with no one-to-one native tag; those typed entry
destinations are not unexpected target findings.

Automatic `SH`, `SS`, `Sh`, and `Ss` tags are excluded from literal comparison
because ManT intentionally uses the complete visible heading for section
identity, while libmandoc's formatter tag may use a shortened spelling. An
explicit `.Tg` moved onto the same wrapper remains an exact obligation.
Redirect-only alias pages are recorded but have no local IR destination
obligation.

Rows are keyed by `(corpus, path, decompressed-source SHA-256)`. `clean` needs
no review. Every `review` or `hard-failure` row starts as `pending` and must be
classified as `false-positive`, `confirmed-open`, or `confirmed-fixed` with an
explanatory note. A confirmed product defect becomes a redistributable real
fixture and a focused Rust test.

## Current complete sweep

On 2026-09-04 the complete local Arch Linux manual hierarchy was rescanned at
producer commit `c51be9d8a539080dd8f53e8812386d8c7b4f782e` with the v3
occurrence-aware contract. The profiler binary SHA-256 was
`6c6ada2e38de0bd4dab5c0a692c50fd94e599103b472505f339457be6bc6cf70`:

```sh
cargo build --locked -p mant-engine --example roff_target_profile
python3 scripts/audit-roff-targets.py --manpath /usr/share/man \
  --corpus archlinux-host --recheck-recorded --findings-only \
  --json /tmp/mant-target-archlinux-v3-c51be9d.json
```

The sweep examined 28,712 pages and produced 28,712 clean results. All 565,090
raw owners formed 565,090 logical obligations; every owner was classified and
every retained obligation matched exactly one compatible IR occurrence. There
were no missing or unexpected targets, role collisions, invalid
identities/fragments, duplicates, dangling links, or unclassified owners. The
owner census exercised man and mdoc section, paragraph, definition, list,
display, inline, function, command, variable, error, and literal forms.
Historical v1/v2 rows remain for source identities absent from the current
host, while every page present in this complete sweep and every checked-in
fixture now has a v3 record.

CI does not depend on `/usr/share/man`. It builds the profiler and verifies the
small checked-in fixture corpus against recorded rows:

```sh
python3 scripts/audit-roff-targets.py --fixtures --recheck-recorded \
  --verify --findings-only
```
