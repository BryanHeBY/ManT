"""Section audit must bind each template to its admitted native branch."""

from collections import Counter
from copy import deepcopy
import json
from unittest import TestCase

from scripts.roff.audit.word_row_matrix.manifest import SECTION_MEMBERS
from scripts.roff.audit.word_row_matrix.oracle import stream_bytes
from scripts.roff.audit.word_row_matrix.section_observations import branch_witness, check_section
from scripts.roff.audit.word_row_matrix.sections import section_sources


def prior_owner_probe(payload, native_rows, following_payload=None):
    # These named rows were checked with the pristine man driver at PD 0;
    # preceding TEXT and SS HEAD share a tcol until post_SH runs term_newln.
    following = f"      {following_payload} (text) *4:1 NOFILL\n" if following_payload else ""
    heading_line = 5 if following_payload else 4
    tree = f"""SH (block) *1:2
  SH (head) 1:2
      FIRST (text) 1:5
  SH (body) 1:2
      PD (elem) *2:2
          0 (text) 2:5
      {payload} (text) *3:1 NOFILL
{following}      SS (block) *{heading_line}:2
        SS (head) {heading_line}:2
            NEXT (text) {heading_line}:5
        SS (body) {heading_line}:2
            AFTERMARK (text) *{heading_line + 1}:1
""".encode()
    # Preserve the native backspace markup to exercise the actual device decoder.
    native = dict(code=0, stdout=stream_bytes(native_rows.encode()))
    return tree, native


def paragraph(value, line):
    return dict(type="paragraph", children=[dict(type="text", value=value)],
                source=dict(line=line))


def observe_section(heading, rows, prior=None, heading_line=4):
    children = []
    for index, row in enumerate(heading.split("\n")):
        if index:
            children.append(dict(type="line-break"))
        if row:
            children.append(dict(type="text", value=row))
    target = dict(source=dict(line=heading_line), heading=dict(content=children),
                  children=[], blocks=[paragraph("AFTERMARK", heading_line + 1)])
    first = dict(source=dict(line=1), children=[target], blocks=prior or [])
    observation = dict(json=json.dumps(dict(document=dict(sections=[first]))), plain=rows,
                       validDocument=True, jsonEquivalent=True, ansiEquivalent=True)
    return first, target, observation


class SectionWordMatrixTests(TestCase):
    def test_logical_members_are_not_deduplicated_before_the_native_run(self):
        records = list(section_sources())
        self.assertEqual(Counter(cohort for cohort, _, _ in records), SECTION_MEMBERS)
        self.assertEqual(len({(cohort, tuple(axes)) for cohort, axes, _ in records}), len(records))
        self.assertLess(len({source for _, _, source in records}), len(records))

    def test_native_transparency_depends_on_macro_and_tree_role(self):
        # The fixed profiles in the section matrix also record each complete
        # tree. This unit test isolates the machine-readable branch witness.
        for macro, token, action, gap in (
            ("SH", "ft", "none", 0), ("Sh", "Sm", "vertical-row", 1),
            ("SS", "ft", "none", 0), ("Ss", "Sm", "vertical-row", 1),
        ):
            with self.subTest(macro=macro):
                tree = (f"{macro} (block) *1:2\n"
                        f"  {macro} (head) 1:2\n      PREVIOUS (text) 1:5\n"
                        f"  {macro} (body) 1:2\n      {token} (elem) *2:2\n"
                        f"{macro} (block) *3:2\n"
                        f"  {macro} (head) 3:2\n      NEXT (text) 3:5\n"
                        f"  {macro} (body) 3:2\n").encode()
                witness = branch_witness(tree)
                self.assertEqual([child["label"] for child in witness["rawBodyChildren"]], [token])
                self.assertEqual(witness["filteredBodyChildren"], [])
                self.assertEqual(witness["action"], action)
                self.assertEqual(witness["expectedGap"], gap)

    def test_no_print_and_transparent_predecessors_do_not_create_an_ss_gap(self):
        tree = b"""Sh (block) *1:2
  Sh (head) 1:2
      FIRST (text) 1:5
  Sh (body) 1:2
      Sm (elem) *2:2
          off (text) 2:5
      No (elem) *3:2 NOPRT
      Ss (block) *4:2
        Ss (head) 4:2
            NEXT (text) 4:5
        Ss (body) 4:2
"""
        witness = branch_witness(tree)
        self.assertIsNone(witness["previous"])
        self.assertEqual(witness["action"], "newline")
        self.assertEqual(witness["expectedGap"], 0)

    def test_native_same_row_still_checks_preceding_payload_owner(self):
        native = prior_owner_probe(r"ALPHA\c", "FIRST\n   ALPHAN\bNE\bEX\bXT\bT\n     AFTERMARK\n")
        old = dict(paragraph("ALPHA", 3), type="preformatted")
        first, target, observation = observe_section("NEXT", "FIRST\nALPHA\nNEXT\nAFTERMARK", [old])
        for axis in ("prior-content", "prior-source", "prior-rendered"):
            self.assertEqual(check_section(*native, observation)["axes"][axis], ("exact",))
        rendered_loss = dict(observation, plain="FIRST\nNEXT\nAFTERMARK")
        self.assertEqual(check_section(*native, rendered_loss)["axes"]["prior-rendered"][0],
                         "difference")
        first["blocks"] = []
        for moved in (False, True):
            if moved:
                target["blocks"].insert(0, old)
            observation["json"] = json.dumps(dict(document=dict(sections=[first])))
            observation["plain"] = "FIRST\nNEXT\nAFTERMARK" + ("\nALPHA" if moved else "")
            result = check_section(*native, observation)
            self.assertEqual(result["axes"]["heading"][0], "exact")
            self.assertEqual(result["axes"]["prior-content"][0], "difference")
            self.assertEqual(result["state"], "difference")

    def test_buffered_zero_advance_can_enter_the_heading(self):
        native = prior_owner_probe(r"\zX", "FIRST\n   XN\bNE\bEX\bXT\bT\n     AFTERMARK\n")
        _, _, observation = observe_section("XNEXT", "FIRST\nXNEXT\nAFTERMARK")
        for axis in ("heading", "prior-content", "prior-source"):
            self.assertEqual(check_section(*native, observation)["axes"][axis], ("exact",))
        wrong = deepcopy(observation)
        wrong["json"] = wrong["json"].replace("XNEXT", "NEXT")
        wrong["plain"] = wrong["plain"].replace("XNEXT", "NEXT")
        self.assertEqual(check_section(*native, wrong)["axes"]["prior-content"][0], "difference")

    def test_rejected_heading_still_owns_one_native_row(self):
        native = prior_owner_probe(r"\p", "FIRST\n\n     AFTERMARK\n")
        first, target, observation = observe_section("\n", "FIRST\n\nAFTERMARK")
        self.assertEqual(check_section(*native, observation)["axes"]["heading"], ("exact",))
        self.assertEqual(check_section(*native, observation)["axes"]["suppressed-heading-rows"],
                         ("exact",))
        target["heading"]["content"] = []
        observation["json"] = json.dumps(dict(document=dict(sections=[first])))
        self.assertEqual(check_section(*native, observation)["axes"]["suppressed-heading-owner"][0],
                         "difference")
        target["spacingBeforeLines"] = 1
        observation["json"] = json.dumps(dict(document=dict(sections=[first])))
        self.assertEqual(check_section(*native, observation)["axes"]["suppressed-heading-owner"],
                         ("exact",))
        self.assertEqual(check_section(*native, observation)["axes"]["entry-gap"][0], "review")
        observation["plain"] = "FIRST\nAFTERMARK"
        self.assertEqual(check_section(*native, observation)["axes"]["suppressed-heading-rows"][0],
                         "difference")

    def test_repeated_invisible_rows_cannot_move_to_the_wrong_owner(self):
        native = prior_owner_probe(r"\p\&", "FIRST\n\n\n   N\bNE\bEX\bXT\bT\n     AFTERMARK\n",
                                  following_payload=r"\p\&")
        old = dict(paragraph("", 3), type="preformatted")
        _, _, correct = observe_section("\nNEXT", "FIRST\n\n\nNEXT\nAFTERMARK", [old], 5)
        _, _, moved = observe_section("\n\nNEXT", correct["plain"], heading_line=5)
        self.assertEqual(check_section(*native, correct)["axes"]["invisible-owner"], ("exact",))
        verdict = check_section(*native, moved)
        self.assertEqual(verdict["axes"]["before-heading"], ("exact",))
        self.assertEqual(verdict["axes"]["invisible-owner"], ("difference", (1, 1), (0, 2)))
