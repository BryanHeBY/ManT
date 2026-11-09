"""The finite word-state matrix retains independent exact oracle receipts."""
from collections import Counter
import gzip
import itertools
import json
import unittest

from scripts.roff.fixtures import record_word_spacing as recorder
from scripts.roff.fixtures import record_word_passes as passes
from scripts.roff.fixtures import record_word_projection as projection
from scripts.roff.fixtures import record_word_receipts as receipts


class WordSpacingFixtureTests(unittest.TestCase):
    def test_ordered_marker_pass_matrix_keeps_every_source_and_profile(self):
        matrix = json.loads(gzip.decompress(passes.FIXTURE.read_bytes()))
        self.assertEqual(matrix["terminal_width"], 160)
        self.assertEqual(matrix["oracle_sha256"],
                         "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e")
        cases = matrix["cases"]
        self.assertEqual(Counter(case["cohort"] for case in cases),
                         {"cross": 768, "wrapper": 324, "consumer": 31})
        self.assertEqual(len({case["source_sha256"] for case in cases}), len(cases))
        self.assertEqual(Counter(case["profiles"]["lint"][0] for case in cases),
                         {0: 931, 1: 192})
        for expected, actual in zip(passes.specifications(), cases, strict=True):
            with self.subTest(case=actual["id"]):
                self.assertEqual(expected, {key: actual[key] for key in expected})
                self.assertEqual(receipts.digest(actual["source"].encode()), actual["source_sha256"])
                self.assertEqual(set(actual["profiles"]), set(receipts.PROFILES))
                self.assertTrue(actual["tree_words"])

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

    def test_source_receipt_matrix_keeps_its_full_cross_and_exact_profile_identity(self):
        fixture = json.loads(gzip.decompress(receipts.FIXTURE.read_bytes()))
        self.assertEqual(fixture["terminal_width"], 160)
        self.assertEqual(
            fixture["oracle_sha256"],
            "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e")
        cases = fixture["cases"]
        self.assertEqual(Counter(case["cohort"] for case in cases), {
            "W1": 630, "W1-control": 20, "W2": 23, "W3": 45, "W4": 19,
            "W5": 5, "W6": 13, "consumer": 3,
        })
        self.assertEqual(len({case["id"] for case in cases}), len(cases))
        self.assertEqual(len({case["source_sha256"] for case in cases}), 754)
        for expected, actual in zip(receipts.specifications(), cases, strict=True):
            with self.subTest(case=actual["id"]):
                self.assertEqual(expected, {key: actual[key] for key in expected})
                self.assertEqual(receipts.digest(actual["source"].encode()), actual["source_sha256"])
                self.assertEqual(set(actual["profiles"]), set(receipts.PROFILES))
                for profile, (status, stdout, stderr) in actual["profiles"].items():
                    expected_status = int(profile == "lint" and actual["comparison"] == "device-tab") * 2
                    self.assertEqual(status, expected_status)
                    self.assertRegex(stdout, r"^[0-9a-f]{64}$")
                    self.assertRegex(stderr, r"^[0-9a-f]{64}$")

    def test_word_projection_matrix_keeps_its_native_witnesses_and_comparison_axes(self):
        fixture = json.loads(gzip.decompress(projection.FIXTURE.read_bytes()))
        self.assertEqual(fixture["terminal_width"], 160)
        self.assertEqual(
            fixture["oracle_sha256"],
            "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e")
        cases = fixture["cases"]
        self.assertEqual(Counter(case["cohort"] for case in cases), {
            "reported-prefixes": 576, "core": 1638, "scope": 42, "field": 21,
            "separator": 138, "separator-suffix": 20, "separator-sentence": 10,
            "tight-empty": 18, "consumer": 7,
        })
        self.assertEqual(len({case["source_sha256"] for case in cases}), len(cases))
        self.assertEqual(Counter(case["profiles"]["lint"][0] for case in cases),
                         {0: 2265, 1: 4, 2: 201})
        for expected, actual in zip(projection.specifications(), cases, strict=True):
            with self.subTest(case=actual["id"]):
                self.assertEqual(expected, {key: actual[key] for key in expected})
                self.assertEqual(receipts.digest(actual["source"].encode()), actual["source_sha256"])
                self.assertEqual(set(actual["profiles"]), set(receipts.PROFILES))
                self.assertTrue(actual["tree_words"])
                for profile, (status, stdout, stderr) in actual["profiles"].items():
                    if profile != "lint":
                        self.assertEqual(status, 0)
                    self.assertRegex(stdout, r"^[0-9a-f]{64}$")
                    self.assertRegex(stderr, r"^[0-9a-f]{64}$")


if __name__ == "__main__":
    unittest.main()
