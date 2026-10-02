# Definition row and word relations

`cases.json` freezes 96 exact sources, their raw profile hashes/statuses and
UTF-8 physical rows from the pristine CVS 20260927T130954Z oracle. The producer
identity is recorded in the fixture; the formal archive/recipe and attestation
are maintained under `libmandoc-rs/upstream/oracle/`. All source inputs ran the
reference before the assertions were written. Native parsing also checks each
BODY marker's actual It HEAD/BODY ancestry during the Rust test.

The first 48 inputs cover four list kinds, four accepted labels and filled,
literal and continued rows. The remaining 48 cover final HEAD rows, leading
empty/zero-width fields, explicit controls and mode changes. All ASCII/UTF-8,
tree and lint runs succeed. Eight mode-change inputs hit an assertion in
pristine HTML; their terminal assertions remain useful, but their HTML profile
is explicitly failed and is never counted as capability acceptance. The other
88 have five successful profiles. Do not erase or turn those failures into
successful observations when updating the fixture.

The Rust reading assertion preserves empty rows and joined versus separated
words. It permits responsive left origins and repeated device padding, and
makes no claim of exact native columns. Separate IR/render/UI tests assert the
resolved preferred alignment, relative origin composition, grapheme cells,
click/copy positions and literal continuations. Markdown fence formatting is
checked independently from source hard rows and definition ownership.

The Python fixture gate regenerates all 96 Cartesian source identities and
checks exact profile invocations, both raw stream hashes and partial-profile
identities. A reviewed canonical observation seal also prevents independent
row/hash mutation. It certifies snapshot integrity, not a fresh oracle run.
Updates require exact reference runs, actual AST ownership and independent
review; candidate output is not gold.
