# Roff target-conservation audit

`TARGET_AUDIT.csv` records the independent zero-width navigation audit. Visible
fidelity, AST topology, CommonMark projection, and renderer layout can all look
correct after an anchor disappears, so none of those ledgers substitutes for
this one.

## Oracle and scope

The development-only `roff_target_profile` example parses each page twice:
once through the owned `libmandoc-rs` AST and once through ManT's complete
source-aware lowering path. Validated libmandoc deep-link owners form the
expected set. Section IDs, semantic-entry IDs, and inline anchors form the
observed set. Explicit `.Tg` destinations must survive exactly; generated
destinations may use ManT's deterministic normalized collision suffix.

Automatic `Sh` and `Ss` tags are excluded from literal comparison because
ManT intentionally uses the complete visible heading for section identity,
while libmandoc's formatter tag may use a shortened spelling. An explicit
`.Tg` moved onto the same wrapper remains an exact obligation. Redirect-only
alias pages are recorded but have no local IR destination obligation.

Rows are keyed by `(corpus, path, decompressed-source SHA-256)`. `clean` needs
no review. Every `review` or `hard-failure` row starts as `pending` and must be
classified as `false-positive`, `confirmed-open`, or `confirmed-fixed` with an
explanatory note. A confirmed product defect becomes a redistributable real
fixture and a focused Rust test.

## Current complete sweep

On 2026-09-03 the complete local Arch Linux manual hierarchy was scanned:

```sh
cargo build --locked -p mant-engine --example roff_target_profile
python3 scripts/audit-roff-targets.py --manpath /usr/share/man \
  --corpus archlinux-host --recheck-recorded --findings-only \
  --json /tmp/mant-target-archlinux.json
```

The sweep examined 28,674 pages. It produced 28,666 clean results and eight
native parse failures, all manually classified as false positives because the
`libxau 1.0.12-1` package shipped unresolved redirect placeholders of the form
`.so man3/Xau.__libmansuffix__`. There were no unresolved review rows after the
structural-owner and function-owner fixes. The owner census exercised man and
mdoc section, paragraph, definition, list, display, inline, and function forms,
including `Sh`, `Ss`, `Pp`, `IP`, `TP`, `TQ`, `Bl`, `It`, `Bd`, `D1`, and `Fo`.

CI does not depend on `/usr/share/man`. It builds the profiler and verifies the
small checked-in fixture corpus against recorded rows:

```sh
python3 scripts/audit-roff-targets.py --fixtures --recheck-recorded \
  --verify --findings-only
```
