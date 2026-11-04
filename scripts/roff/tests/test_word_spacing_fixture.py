"""The finite word-state matrix retains independent exact oracle receipts."""
from collections import Counter
import itertools
import json
import unittest

from scripts.roff.fixtures import record_word_spacing as recorder


class WordSpacingFixtureTests(unittest.TestCase):
    def test_all_core_axes_and_independent_control_rows_are_recorded(self):
        fixture = json.loads(recorder.FIXTURE.read_text())
        cases = fixture["cases"]
        self.assertEqual(Counter(case["cohort"] for case in cases),
                         {"core": 30, "control": 18, "risk": 44, "flush": 18})
        self.assertEqual(len({case["id"] for case in cases}), 110)
        self.assertEqual(len({case["source_sha256"] for case in cases}), 109)
        self.assertEqual(fixture["distinct_source_count"], 109)
        generated = list(recorder.specifications())
        self.assertEqual(len(generated), len(cases))
        for expected, recorded in zip(generated, cases):
            with self.subTest(case=expected["id"]):
                self.assertEqual(expected["id"], recorded["id"])
                self.assertEqual(expected["source"], recorded["source"])
                self.assertEqual(recorder.digest(recorded["source"]), recorded["source_sha256"])
                self.assertEqual(set(recorded["profiles"]), set(recorder.PROFILES))
                for profile in recorded["profiles"].values():
                    self.assertEqual(profile["status"], 0)
                    self.assertEqual(profile["diagnostics"], "")
                    self.assertRegex(profile["stdout_sha256"], r"^[0-9a-f]{64}$")
                    self.assertRegex(profile["stderr_sha256"], r"^[0-9a-f]{64}$")
        by_id = {case["id"]: case for case in cases}
        for word, transition, successor in itertools.product(
                recorder.WORDS, ("on", "toggle"), recorder.SUCCESSORS):
            case = by_id[f"core-{word}-{transition}-{successor}"]
            self.assertNotIn(" Ns", case["source"])
        for identifier, row in [("core-empty-on-text", "SECOND"),
                                ("core-bare-z-on-text", "ECOND"),
                                ("control-on-empty-on", " SECOND"),
                                ("control-off-nbrzw-on", " SECOND"),
                                ("control-off-size-on", " SECOND"),
                                ("control-on-empty-off", " SECOND")]:
            self.assertEqual(by_id[identifier]["rows"], ["ORIGIN", row, "TAIL"])
        self.assertEqual(by_id["flush-on-empty-on"]["rows"],
                         ["ORIGIN", "call();", "  SECOND", "TAIL"])
        self.assertEqual(by_id["flush-on-empty-br-on"]["rows"],
                         ["ORIGIN", "call();", " SECOND", "TAIL"])

    def test_origin_projection_keeps_all_authored_spaces_and_physical_rows(self):
        raw = b"header\n     ORIGIN\n     \n      SECOND  \n     TAIL\nfooter\n"
        self.assertEqual(recorder.projected_rows(raw), ["ORIGIN", "", " SECOND  ", "TAIL"])


if __name__ == "__main__":
    unittest.main()
