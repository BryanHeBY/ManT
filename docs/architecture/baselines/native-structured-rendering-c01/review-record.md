# C01 cross-review record

Base: `6a1469a4`

Final contract SHA-256:
`6125e6a59db955fcef86a50c73b2a92f77e6b00a3b060d87e8f615ebdebb56b9`

The first complete draft (`662b5365...a42f6995`) received independent N, F,
and S read-only review. Findings required an executable minimum FFI shape,
per-column sidecar transitions, explicit budgets and ownership, source-coordinate
units, public connection/point/link types, exact entry refs, grapheme/column
maps, SourceFormat/source contexts, and distinct logical/visual consumer paths.

A later candidate (`c3bcf5c5...350ae67`) closed those findings. Targeted review
then found input-slot/result-key ambiguity, an incomplete resolver/topology,
duplicate public inline text, an incomplete second entry form, a missing
decoration display target, and remaining request-side Markdown search authority.

The final candidate closes those issues by using one public atom store,
string-free inline leaves, complete form/name refs, separate InputSourceSlot and
result SourceKey domains, a current-source resolver contract, decoration and
profile-glyph display mappings, complete collection/transfer budgets, and
logical search with Markdown projections. Final targeted dispositions:

| Review | Result | Scope |
| --- | --- | --- |
| N: native contract | PASS | Pinned parser/term/field paths, six connection cases, source entry, owner stack, direct output, fixed/flow boundary |
| F: FFI/resources | PASS | Input/result key domains, resolver status, view/topology IDs, limits, ownership, failure cleanup, ABI admission boundary |
| S: semantics/consumers | PASS | Single body, entry refs, points/decorations, link occurrence, search/query/copy/display projections, strict v0.12 rewrite |

The main agent reran registered oracle preflight for ASCII and UTF-8 against the
frozen archive/attestation/binary. All baseline JSON files parse, the NDJSON has
24 records (three rounds for two revisions and four operations), and every
artifact checksum listed in `README.md` matches. N independently reran UTF-8
preflight and checksum verification during review.

This record is design/evidence admission only. No structured producer exists
yet. The C00b product test results are recorded in the contract; product tests
were not rerun for this documentation-only C01 candidate. Sanitizers, native
Windows/macOS execution, failure injection, and future sidecar performance
remain unverified.
