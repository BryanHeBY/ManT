# Roff target-conservation audit

This checkout retains only reproducible fixture records. The complete historical
ledger and review narrative are preserved in [mant-roff-corpus](https://github.com/BryanHeBY/mant-roff-corpus/tree/main/audits/mant-tracked/tests/fixtures/roff/TARGET_AUDIT.md).
See [corpus ownership and audit paths](README.md) before running a distribution scan.

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
Some native automatic tags retain source escapes after validation. The oracle
keeps that raw spelling as evidence and independently decodes reviewed
zero-width, sub-column spacing, literal-backslash, hyphen and font-switch forms for canonical-ID
comparison. It does not call the production target decoder or relax owner,
role, occurrence or source-line matching. An unrecognized escape becomes an
unclassified obligation rather than an assumed match. Explicit `.Tg` fragments
are not subjected to this automatic-target spelling rule.
When no native tag string was retained, the fallback takes the first visible
word without splitting a source escape at its encoded space. An actual retained
multiword tag instead keeps its complete spelling; these are distinct cases.
Redirect-only alias pages are recorded but have no local IR destination
obligation.

Rows are keyed by `(corpus, path, decompressed-source SHA-256)`. `clean` needs
no review. Every `review` or `hard-failure` row starts as `pending` and must be
classified as `false-positive`, `confirmed-open`, or `confirmed-fixed` with an
explanatory note. A confirmed product defect becomes a redistributable real
fixture and a focused Rust test.
