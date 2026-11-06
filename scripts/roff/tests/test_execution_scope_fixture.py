"""Source execution fixtures retain independent native identities and witnesses."""

from collections import Counter
import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/execution_scopes/cases.json"


class ExecutionScopeFixtureTests(unittest.TestCase):
    def setUp(self):
        self.fixture = json.loads(FIXTURE.read_bytes())

    def test_sources_profiles_and_original_core_domains_are_bound(self):
        cases = self.fixture["cases"]
        self.assertEqual(len(cases), self.fixture["case_count"])
        self.assertEqual(len({case["id"] for case in cases}), len(cases))
        self.assertEqual(self.fixture["profile_count"], len(cases) * 5)
        counts = Counter(case["family"] for case in cases)
        self.assertEqual(counts["M1-core"], 96)
        self.assertEqual(counts["M2"], 120)
        self.assertEqual(counts["M3"], 20)
        for case in cases:
            self.assertTrue(any(case["expected"].get(key) for key in
                                ("prose_tokens", "glyphs", "table_owners", "authored_prefix")),
                            f"{case['id']}: no active behavioral assertion")
            self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                             case["source_sha256"])
            self.assertEqual(set(case["profiles"]),
                             {"ascii", "utf8", "html", "tree", "lint"})
            for profile, receipt in case["profiles"].items():
                self.assertRegex(receipt["stdout_sha256"], r"^[0-9a-f]{64}$")
                self.assertRegex(receipt["stderr_sha256"], r"^[0-9a-f]{64}$")
                if profile in case.get("reference_failures", {}):
                    self.assertEqual(receipt["status"], case["reference_failures"][profile])
                    self.assertEqual(profile, "html")
                    self.assertEqual(receipt["status"], -11)
                    self.assertIn("pristine HTML crashes", case["reason"])
                elif profile != "lint":
                    self.assertEqual(receipt["status"], 0)

    def test_consumer_registration_does_not_promote_unexecuted_axes(self):
        cases = self.fixture["cases"]
        for case in cases:
            registered = set(case["registered_consumer_checks"])
            uncovered = set(case["uncovered_consumer_axes"])
            self.assertFalse(registered & uncovered)
            self.assertIn("whole-page-font-and-device-geometry", uncovered)
            self.assertIn("pty-cli-40-78", uncovered)
        self.assertEqual(sum("cli-pipe-plain-and-ansi" in case["registered_consumer_checks"]
                             for case in cases), 236)
        self.assertEqual(sum("actual-buffer-40-78" in case["registered_consumer_checks"]
                             for case in cases), 164)

    def test_native_witnesses_do_not_conflate_markers_or_container_tables(self):
        cases = {case["id"]: case for case in self.fixture["cases"]}
        nested = cases["m3-tag-enum-d1"]["expected"]["glyphs"]
        self.assertEqual(nested["DISPLAY"] - nested["AFTER"], 6)
        self.assertEqual(nested["BEFORE"], nested["AFTER"])
        column = cases["m2-column-text-rule"]["expected"]["table_owners"]
        self.assertEqual(len(column), 1)
        self.assertGreater(column[0]["first_span_line"], column[0]["ts_line"])
        self.assertEqual([row["kind"] for row in column[0]["rows"]],
                         ["data", "horizontal-rule", "data"])
        empty = cases["m2-tag-none-empty-last"]["expected"]["table_owners"][0]
        self.assertEqual(empty["declared_columns"], 2)
        self.assertEqual(empty["rows"][0]["cells"], ["A"])
        self.assertEqual(empty["rows"][0]["implicit_empty_columns"], [1])

    def test_styled_native_cells_and_required_interactions_remain_explicit(self):
        cases = {case["id"]: case for case in self.fixture["cases"]}
        # tree.c::print_tbl emits optional font/flag letters after each
        # positional code: 0Lb[A] is a real cell, not absent topology.
        for context in ("tag", "inset"):
            owner = cases[f"table-shape-{context}-font"]["expected"]["table_owners"][0]
            self.assertEqual([row["cells"] for row in owner["rows"]],
                             [["A", "B"], ["C", "D"]])
        closure = [case for case in cases.values() if case["family"] == "MX-closure"]
        self.assertEqual(Counter(case["axes"]["interaction"] for case in closure),
                         {"X01": 2, "X02": 2, "X07": 6})
        self.assertEqual({case["axes"]["control"] for case in closure
                          if case["axes"]["interaction"] == "X07"},
                         {"c", "bare-z", "font"})

    def test_warning_recovery_sources_remain_in_the_domain(self):
        warning = [case for case in self.fixture["cases"]
                   if case["profiles"]["lint"]["status"] != 0]
        self.assertTrue(warning)
        self.assertTrue(all(case["native_status"] == "CapturedWithDiagnostics"
                            for case in warning))


if __name__ == "__main__":
    unittest.main()
