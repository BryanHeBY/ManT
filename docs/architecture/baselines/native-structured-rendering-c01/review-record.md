# C01 cross-review record

Base: `6a1469a4`

Final contract SHA-256:
`a791ae397f90f4044e4788e4bff3f430fb2de497c14be25858953594c49e5551`

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

## C04 address and wire amendment

Date: 2026-09-22

Base: `72fc0877`

C04 exposed four relationships that the original C01 freeze did not make
executable: generic native anchor identity at a `ContentPoint`, authored
heading identity distinct from display text, hard breaks inside a link label,
and self-contained content for partial protocol responses. The amendment adds
native-only anchor/heading evidence, ordered content/hard-break link-label
parts, ABI v4 views and limits, and a bounded response-local
`ContentProjection`. It also freezes root-local logical search, cross-root
occurrence identity, deterministic public-ID/alias allocation, and atomic
section-link downgrade.

Targeted read-only N and S reviews both passed after the final corrections. N
checked enum widths, optional heading evidence, global atom/link ordering,
HardBreak integrity, budgets, and byte accounting. S checked root-local search,
Section/Entry landing-point ownership, alias collision priority, fallback IDs,
projection closure and budgets, reference counting, and response-local key
domains. This was a documentation-only amendment: no product build or test was
run for it, and implementation remains the work of the corresponding C09a/C04
subunits.
