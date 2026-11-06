# Name BODY row origins

These two source-bound controls are separate from the sealed 45-case normal
item/name matrix. They preserve the exact minimal single-leading-row and
repeated-leading-row inputs that exposed a lost continuation layout and an
earlier accepted origin being overwritten by a later source-row commit.

Before the assertions were written, both complete inputs ran the locked
pristine reference in ASCII and UTF-8 at widths 40/78, plus HTML, tree and lint:
14 successful receipts. Only the fixed five-column section origin and device
trailing padding are removed. The seven leading spaces in each recorded TEXT
row include six columns of Nm BODY origin and one **authored** source space.
No per-row left trimming, source-space removal or candidate-derived golden
value is used.

Pinned `mdoc_term.c::termp_nm_pre()` assigns the BODY offset from the original
name width plus one. `print_mdoc_node()` executes leading TEXT line boundaries
in source order and restores source geometry at node return. Accepted row
origins and paragraph continuation origins therefore describe distinct facts:
the second hard-row commit cannot move an earlier row, and retiring one IR
owner cannot erase the six-column continuation preference.

Engine checks bind actual Nm BODY AST ownership, original line/column slices,
the complete paragraph source owner, exact authored inline text, JSON string
roundtrip, both resolved hard-row origins, continuation layout and CLI rows.
Two real Markdown readbacks preserve all hard rows and the one authored source
space; ordinary Markdown intentionally omits reading alignment. UI checks paint
both terminal widths in all three rounds, including exact native fonts, search
cell ranges, absence of invented links and actual pointer-selection copy of
the authored leading space. Generated layout padding is not copied as text.

```sh
python3 -m scripts.roff.fixtures.record_name_body_origins \
  --check --evidence target/audits/name-body-origin-oracle
python3 -m unittest scripts.roff.tests.test_name_body_origin_fixture
```
