# Accepted code rows and original logical identity

This source-neutral fixture protects the frozen v0.13 IR contract during
Markdown projection. It is not a roff execution or terminal-geometry oracle.
Original logical rows are counted by authored LF and `Inline::LineBreak`;
code-role bare CR creates an accepted fragment of the same original row.

## Core domain

`cases.json` contains **192 complete, valid hand-IR JSON inputs**:

- Six shapes: LF, CRLF, bare CR before a real LF, multiple bare CR before real
  LF, bare-CR open tail, real-LF open tail.
- Four hint policies: none, common indentation, first-line-only correction,
  common plus first-line correction plus alignment.
- Four fragment layouts: one Text, scalar-split Text, alternating Strong and
  Emphasis, separate nonnested External Link labels and zero-width Anchors.
- Two locations: ordinary Preformatted, Preformatted in a table cell.

Splits deliberately separate CR from LF across leaves and wrappers. The six
source-row/accepted-fragment witnesses are enumerated independently of the
product's normalizer; expected text applies the original row's common origin
to all fragments, and its first-line correction to the first fragment only.
Viewport alignment is retained on the original IR but does not add guessed
viewport padding to portable fenced text. Empty accepted rows receive no cells.

Every input has a SHA-256 of its exact embedded JSON source, axes, original
text/row count and explicit accepted row witnesses. The fixture is static;
expected values are not rebuilt from encoder output. The test checks the
public `logical_row_count`, layout validity and `RowLayoutHint::resolve` witness
before exporting, then checks:

1. Validator and actual JSON-string round trip.
2. Public fragment bytes via the locked CommonMark parser's actual code-block
   payload, independently of ManT's Markdown reader.
3. Addressable artifact/String parity with both anchor policies.
4. Two actual Markdown reads, each against the independent accepted payload.
5. Original IR/layout/typed-link facts remain unchanged.

The matrix does not require code-fence text to retain active links: the source
IR's link facts are preserved, while the portable literal projection retains
its visible label as code. Nested active links and row hints beyond the
original logical row count are invalid and are not counted as legal cases.

## Mandatory boundaries

`boundaries.rs` retains the six original reports and adds:

- Common and first-line positive/negative corrections, same-row CR fragments,
  later source rows, multiple hints and Unicode/emoji/combining characters.
- Cross-leaf CRLF through styles, empty Text and Anchor; explicit LineBreak is
  an independent authored event and never coalesces with a preceding CR.
- Reading Text CR -> replacement versus true Code-role CR -> LF; crossing the
  role boundary, Inline Code and Unsupported's existing reading/edge policy.
- Deferred open-tail origins across a following table cell: bare CR stays on
  the original source row, CRLF addresses the following logical row, and a
  first-line-only correction does not leak into a CR continuation.
- Safe Unicode spaces/separators, control masking, authored empty literal and
  navigation-only roots without invented code blocks or tail padding.

`owners.rs` combines two semantic owners in a table. CRLF shrinkage, CR growth,
Unicode, variable fence widths and anchor syntax must preserve exact final
UTF-8 owner slices. The FIRST range cannot include SECOND or framing syntax;
fragment/String/artifact exports share the same accepted payload.

Existing `table_boundaries/content_roles.rs`, `row_layout/literal_rows.rs` and
MAP01 owner tests remain in place. The old bare-CR tail assertion now explicitly
uses the same original row's common origin; its real-LF counterexample remains.
This is a source-identity correction, not permission for layout hints to
manufacture empty text.

No product version/schema, roff fixture expectation or native source changes
are part of this matrix. CLI/TUI safe raw-control behavior is a separate
consumer contract and must not be compared to Markdown's accepted code
normalization as if they promised identical control execution.
