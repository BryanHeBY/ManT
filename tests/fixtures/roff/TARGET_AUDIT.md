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

Profile schema `mant.roff-target-profile/v4` classifies every deep-link owner
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
remain distinct occurrences and are consumed at most once. Anchors additionally
carry their addressable owner's source span, while the profiler records the
exact block, list-item, definition, and table-cell path where each occurrence
landed. A match requires the native and IR owner source lines to agree, so a
target moved to a same-kind sibling in the same section produces both a missing
obligation and an unexpected occurrence. Consequently an unrelated same-named
section/entry, one retained occurrence standing in for two owners, or an anchor
in an incompatible or neighbouring structural owner cannot manufacture a clean
result. The profile reports true unexpected targets, incompatible
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

On 2026-09-04 a broader restored logical inventory was then scanned after the
numbered-paragraph and target-conservation fixes at producer commit
`9233968b21f046bdcf301f5b34954ee43cdd2292`. Its inventory manifest contained
45,036 logical paths backed by 37,286 physical source paths; the manifest
SHA-256 was
`a5cd7919d1ac2774a335d077611b47680de8c1babe1b031d31ff8808f568879c`.
It combined the recorded Alpine, Arch Linux host, Apple OSS, Debian sid,
DragonFly BSD, Fedora, FreeBSD, illumos, NetBSD 10/11, OpenBSD, generated,
current-host, Miniconda, and checked-in fixture inputs. The audit used the
current available bytes at each logical path; the exact decompressed source
digests remain in the local CSV ledger.

The v3 profiler binary SHA-256 was
`ed52581a5a7c2d6a042b0fbb760ae4bf89ac14a0ea7bb017197c00f4b772af1c`.
All 45,036 pages were clean. Their 1,192,621 raw owners formed 1,192,621
logical obligations; every owner was classified and every retained obligation
matched a compatible occurrence. Missing and unexpected targets, role
collisions, invalid identities, duplicate identities, dangling links, and
unclassified raw or logical owners were all zero. The exact reports on the
audit host are `/tmp/mant-target-full-45036-9233968.json` and
`/tmp/mant-target-full-45036-9233968.csv`.

After target derivation was made source-authoritative, the same restored
inventory was rescanned at producer commit
`bd92d5cec46fca550819a5c890e56bbdcf69b8e4`. The v3 profiler binary SHA-256
was `6b5c2cf3db0e22454f5cb1c65f0c79b1e8ed4aef30744c3b7cd8e2a9f9c7ecc3`.
All 45,036 pages were clean: 1,192,621 raw target owners formed exactly
1,192,621 classified logical obligations, and every retained obligation
matched a compatible IR occurrence. All missing, unexpected, role-collision,
invalid-identity, duplicate, dangling, and unclassified counts were zero.
This rerun includes the argument-less `.Tg` forms that can bind either to a
following source owner or to libmandoc's preceding paragraph owner. The exact
reports are `/tmp/mant-target-full-45036-bd92d5c.json` and
`/tmp/mant-target-full-45036-bd92d5c.csv`.

CI does not depend on `/usr/share/man`. It builds the profiler and verifies the
small checked-in fixture corpus against recorded rows:

```sh
python3 scripts/audit-roff-targets.py --fixtures --recheck-recorded \
  --verify --findings-only
```
