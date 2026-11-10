"""Template labels cannot stand in for an observed section branch."""

from unittest import TestCase

from scripts.roff.audit.word_row_matrix.section_ledger import entry_admission


class SectionWordLedgerTests(TestCase):
    def test_empty_peer_is_not_admitted_after_a_state_request(self):
        witness = dict(node={"label": "SS"}, previous={"label": "SS"}, rawBodyChildren=[])
        self.assertEqual(entry_admission("empty-peer", witness), "observed")
        witness["rawBodyChildren"] = [dict(label="nf", kind="elem", noPrint=False)]
        self.assertEqual(entry_admission("empty-peer", witness), "normalized-or-other-branch")

    def test_only_a_matching_peer_can_admit_the_transparent_body_template(self):
        witness = dict(node={"label": "SS"}, previous={"label": "SH"},
                       rawBodyChildren=[dict(label="ft", kind="elem", noPrint=False)])
        self.assertEqual(entry_admission("transparent-peer", witness), "normalized-or-other-branch")
        witness["previous"]["label"] = "SS"
        self.assertEqual(entry_admission("transparent-peer", witness), "observed")
        self.assertEqual(entry_admission("none", None), "uncovered")
