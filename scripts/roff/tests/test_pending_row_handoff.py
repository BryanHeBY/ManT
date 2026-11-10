"""A row verdict must identify both the native action and its source owner."""

from collections import Counter
import json
from unittest import TestCase

from scripts.roff.audit.word_row_matrix.manifest import PENDING_MEMBERS
from scripts.roff.audit.word_row_matrix.oracle import stream_bytes
from scripts.roff.audit.word_row_matrix.pending_handoff import END, HEADER, pending_sources
from scripts.roff.audit.word_row_matrix.pending_observations import check_pending_rows


def probe(word, native_rows, *, authored_br=False, retained_br=False):
    # Pinned man_term.c::pre_SS / post_SH and roff_term.c::roff_term_pre_br.
    # Keep the tree and authored coordinates separate: man validation can
    # remove the br even though its source line precedes the heading.
    body = word + "\n" + (".br\n" if authored_br else "") + ("\\&\n" if retained_br else "")
    source = HEADER + body + ".SS NEXT\nAFTERMARK\n" + END
    line = 6 + len(body.splitlines())
    native_br = "      br (elem) *7:2\n      \\& (text) *8:1\n" if retained_br else ""
    tree = f"""SH (block) *4:2
  SH (head) 4:2 ID=HREF
      FIRST (text) 4:5
  SH (body) 4:2
      PD (elem) *5:2
          0 (text) 5:5
      {word} (text) *6:1
{native_br}      SS (block) *{line}:2
        SS (head) {line}:2 ID=HREF
            NEXT (text) {line}:5
        SS (body) {line}:2
            AFTERMARK (text) *{line + 1}:1
""".encode()
    native = dict(code=0, stdout=stream_bytes(("\n".join(["FIRST", *native_rows, "ENDTEST", ""])).encode()))
    return source, tree, native


def paragraph(text, line):
    return dict(type="paragraph", children=[dict(type="text", value=text)],
                source=dict(line=line))


def observe(probe_receipts, old, title, rows, gap=0):
    source = probe_receipts[0]
    line = source.splitlines().index(".SS NEXT") + 1
    nodes = [dict(type="line-break")] if title.startswith("\n") else []
    if title:
        nodes.append(dict(type="text", value=title.removeprefix("\n")))
    target = dict(id="next", source=dict(line=line), heading=dict(content=nodes),
                  spacingBeforeLines=gap, children=[], blocks=[paragraph("AFTERMARK", line + 1)])
    first = dict(id="first", source=dict(line=4), children=[target], blocks=old)
    observation = dict(json=json.dumps(dict(document=dict(sections=[first]))),
                       plain="\n".join(["FIRST", *rows, "ENDTEST"]), validDocument=True,
                       jsonEquivalent=True, ansiEquivalent=True)
    return check_pending_rows(*probe_receipts, observation)


class PendingRowHandoffTests(TestCase):
    def test_frozen_matrix_keeps_all_reviewed_logical_coordinates(self):
        records = list(pending_sources())
        self.assertEqual(Counter(cohort for cohort, _, _ in records), PENDING_MEMBERS)
        self.assertEqual(len({(cohort, tuple(axes)) for cohort, axes, _ in records}), 648)
        self.assertEqual(len({source for _, _, source in records}), 648)
        self.assertEqual(sum(".br\n" in source for _, _, source in records), 216)

    def test_accepted_word_stays_in_its_owner_and_does_not_supply_a_heading_row(self):
        rows = ["ALPHA", "NEXT", "AFTERMARK"]
        receipt = probe(r"ALPHA\p", rows)
        self.assertEqual(observe(receipt, [paragraph("ALPHA", 6)], "NEXT", rows)["state"],
                         "asserted-axes-pass")
        extra = observe(receipt, [paragraph("ALPHA", 6)], "\nNEXT", rows)
        self.assertEqual(extra["axes"]["heading"], ("difference", "NEXT", "\nNEXT"))
        missing = observe(receipt, [], "NEXT", rows)
        self.assertEqual(missing["axes"]["old-content"], ("difference", ["ALPHA"], []))
        moved = observe(receipt, [paragraph("ALPHA", 7)], "NEXT", rows)
        self.assertEqual(moved["axes"]["old-source"], ("difference", True, False))

    def test_accepted_invisible_pass_is_distinct_from_an_extra_row(self):
        rows = ["", "NEXT", "AFTERMARK"]
        receipt = probe(r"\p\&", rows)
        result = observe(receipt, [], "\nNEXT", rows)
        self.assertEqual(result["state"], "asserted-axes-pass")
        self.assertEqual(result["evidence"]["acceptedInvisiblePass"], "heading-leading-row")
        dropped = observe(receipt, [], "NEXT", rows[1:])
        self.assertEqual(dropped["axes"]["reader-rows"], ("difference", rows, rows[1:]))

    def test_adjacent_br_witness_cannot_be_inferred_from_source(self):
        deleted = probe(r"\p BETA", ["", "AFTERMARK"], authored_br=True)
        result = observe(deleted, [], "", ["", "AFTERMARK"], 1)
        self.assertEqual(result["evidence"]["authoredBr"], [7])
        self.assertEqual(result["evidence"]["nativeBr"], [])
        self.assertEqual(result["state"], "asserted-axes-pass")
        rows = ["", "NEXT", "AFTERMARK"]
        retained = probe(r"\p BETA", rows, authored_br=True, retained_br=True)
        old = [dict(type="vertical-space", lines=1, source=dict(line=7))]
        result = observe(retained, old, "NEXT", rows)
        self.assertEqual(result["evidence"]["nativeBr"], [7])
        self.assertEqual(result["state"], "asserted-axes-pass")
        wrong_owner = observe(retained, [], "NEXT", rows, 1)
        self.assertEqual(wrong_owner["axes"]["completed-old-row"],
                         ("difference", (1, 0), (0, 1)))

    def test_native_shared_device_row_is_never_reported_as_exact_rows(self):
        rows = ["ALPHA", "NEXT", "AFTERMARK"]
        shared = probe(r"ALPHA\p", ["ALPHANEXT", "AFTERMARK"])
        self.assertEqual(observe(shared, [paragraph("ALPHA", 6)], "NEXT", rows)["state"],
                         "allowed-owner-split")
        unknown = probe(r"ALPHA\p", ["new scalar", "NEXT", "AFTERMARK"])
        self.assertEqual(observe(unknown, [paragraph("ALPHA", 6)], "NEXT", rows)["state"],
                         "review")
