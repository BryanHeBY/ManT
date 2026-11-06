# Reference binding identity matrix

The 108 complete sources cross full/collapsed/shortcut references, LF/CRLF/CR,
ordinary/canonical-hard-row/heading entry, and four URI/title equality relations.
The expected decision comes from the locked parser’s effective original
definition **source span**, independently checked before calling the codec.
Equal URI and equal title do not prove equal reference identity.

`cases.json` preserves exact source hashes, axes, expected basis, asserted
consumer scope and expected safe label. It is a permanent test catalogue, not
a claim that a particular binary already ran every configuration. The Rust
regression checks the manifest identity, original and recovered parser spans,
refusal/diagnostics, JSON string round trip and two real Markdown readbacks.

The adjacent named tests retain first-definition, case/whitespace/Unicode,
normal label/title/inline controls, cache isolation, later accepted metadata,
list/table/heading context, and bounded proof/callback tests. They must not
replace these 108 cases. Native formatting is not the Markdown oracle here.

Not every Cartesian product is meaningful: collapsed/shortcut labels are the
reference ID itself; nested active links are invalid; an originally unresolved
reference stays literal. GFM escaped pipes must satisfy the locked table
parser before they count as a valid reference sample. Diagnostics and source
ranges are checked against that input round, not the initial document forever.

## Provenance limits for table-specific labels

The additional 71 complete-source extension catalogue includes clean and
unrelated-taint controls for GFM table reference IDs containing escaped pipes.
Locked `linklabel.rs` removes that escape only inside a table. When the use
itself needs recovery, an ordinary local proof cannot establish the original
table-specific identity. The selected conservative policy retains the safe
label, omits navigation and reports `markdown.recovered-link-target`; it does
not guess the ID by removing a backslash. This is an explicitly tested allowed
refusal, not a claim that every label-only recovery keeps its link. Clean uses
and uses unaffected by unrelated source damage retain their normal target.
Equal-URI/equal-title collision controls remain refused as real identity
changes. No page-wide link-event history or unbounded context replay is added.
