"""Bind literal leading-row policy to independent pristine source facts."""

import copy
import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "tests/fixtures/roff/literal_leading_rows"
ORACLE_SHA = "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"


def cases():
    return json.loads((FIXTURES / "cases.json").read_text(encoding="utf-8"))


def validate_ui_exception(original, exception):
    digest = hashlib.sha256(original["source"].encode()).hexdigest()
    if digest != original["source_sha256"] or digest != exception["source_sha256"]:
        raise ValueError("source identity drift")
    omitted = exception["omit_native_rows"]
    if len(omitted) != 1 or omitted != sorted(set(omitted)):
        raise ValueError("one unique omitted native row required")
    for row in omitted:
        if not 0 <= row < len(original["expected_rows"]) or original["expected_rows"][row] != "":
            raise ValueError("only a recorded empty native row may be omitted")
        if any(position["row"] == row for position in original["body_positions"]):
            raise ValueError("a named body position cannot be omitted")


class LiteralLeadingRowsFixtures(unittest.TestCase):
    def test_legacy_reader_exceptions_are_bound_to_exact_sources_and_row_windows(self):
        directory = ROOT / "crates/mant-engine/tests/roff_lowering/oracle_migrations"
        cards = json.loads((directory / "literal_reading.json").read_text(encoding="utf-8"))
        self.assertFalse(cards["header"]["expectationsFromProduct"])
        self.assertEqual(cards["header"]["oracleSha256"], ORACLE_SHA)
        self.assertEqual(len(cards["cases"]), cards["header"]["count"])
        self.assertEqual(len({case["source"] for case in cards["cases"]}), len(cards["cases"]))
        for case in cards["cases"]:
            self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                             case["sourceSha256"])
            self.assertTrue(case["owner"])
            self.assertLessEqual({"ascii", "utf8", "tree"}, set(case["profiles"]))
            self.assertLessEqual(set(case["profiles"]),
                                 {"ascii", "utf8", "tree", "utf8-40", "utf8-80"})
            for window in case["windows"]:
                self.assertEqual(len(window["omittedRows"]), 1)
                row = window["omittedRows"][0]
                self.assertEqual(window["nativeRows"][row], "")
                self.assertTrue(any(value.strip() for value in window["nativeRows"][row + 1:]))

        directory = ROOT / "crates/mant-ui/src/document/tests/empty_word_columns"
        native = json.loads((directory / "cases.json").read_text(encoding="utf-8"))
        reading = json.loads((directory / "reading_policy.json").read_text(encoding="utf-8"))
        cases_by_id = {case["id"]: case for case in native["cases"]}
        self.assertEqual(len(reading["cases"]), 25)
        self.assertEqual(len({case["id"] for case in reading["cases"]}), 25)
        for case in reading["cases"]:
            original = cases_by_id[case["id"]]
            validate_ui_exception(original, case)

    def test_reader_exception_admission_rejects_source_row_and_position_mutations(self):
        directory = ROOT / "crates/mant-ui/src/document/tests/empty_word_columns"
        native = json.loads((directory / "cases.json").read_text(encoding="utf-8"))
        reading = json.loads((directory / "reading_policy.json").read_text(encoding="utf-8"))
        exception = reading["cases"][0]
        original = next(case for case in native["cases"] if case["id"] == exception["id"])
        for mutation, message in (("source", "source identity"),
                                  ("row", "empty native row"),
                                  ("position", "body position")):
            changed = copy.deepcopy(original)
            if mutation == "source":
                changed["source"] += "\n"
            elif mutation == "row":
                changed["expected_rows"][exception["omit_native_rows"][0]] = " "
            else:
                changed["body_positions"][0]["row"] = exception["omit_native_rows"][0]
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, message):
                validate_ui_exception(changed, exception)

    def test_semantic_seams_keep_exact_sources_and_native_receipts(self):
        directory = FIXTURES / "seams"
        matrix = json.loads((directory / "cases.json").read_text(encoding="utf-8"))
        oracle = json.loads((directory / matrix["oracle"]).read_text(encoding="utf-8"))
        self.assertFalse(matrix["expectationsFromProduct"])
        self.assertEqual(oracle["reference"]["sha256"], ORACLE_SHA)
        recorded = {case["id"]: case for case in oracle["cases"]}
        self.assertEqual(len(recorded), 6)
        for case in matrix["cases"]:
            with self.subTest(case=case["id"]):
                receipt = recorded[case["id"]]
                source = (directory / case["file"]).read_bytes()
                self.assertEqual(hashlib.sha256(source).hexdigest(), case["sourceSha256"])
                self.assertEqual(receipt["sourceSha256"], case["sourceSha256"])
                self.assertTrue(case["nativeTreeFacts"])
                self.assertTrue(case["sourceRules"])
                self.assertEqual(set(receipt["profiles"]), {"ascii", "utf8", "tree", "lint"})
                for profile, output in receipt["profiles"].items():
                    self.assertEqual(output["status"], 0)
                    self.assertEqual(output["arguments"][-1], case["file"])
                    for stream in ("stdout", "stderr"):
                        self.assertEqual(hashlib.sha256(output[stream].encode()).hexdigest(),
                                         output[f"{stream}Sha256"])
                self.assertEqual(receipt["profiles"]["lint"]["stderr"], "")
        tree = recorded["hp_literal_headless_ip"]["profiles"]["tree"]["stdout"]
        self.assertIn("HP (head) 5:2\n        HP (body)", tree)
        self.assertIn("IP (head) 8:2\n        IP (body)", tree)
        self.assertIn("(text) *6:1 NOFILL", tree)

    def test_complete_sources_and_pristine_profiles_have_fixed_identities(self):
        matrix = cases()
        self.assertFalse(matrix["expectationsFromProduct"])
        self.assertEqual(matrix["reference"]["sha256"], ORACLE_SHA)
        self.assertEqual(matrix["reference"]["snapshot"], "20261013T025419Z")
        self.assertEqual(len(matrix["cases"]), 23)
        self.assertEqual(len({case["id"] for case in matrix["cases"]}), 23)
        self.assertEqual({case["file"] for case in matrix["cases"]},
                         {path.name for path in FIXTURES.glob("*.1")})
        recipe = matrix["reference"]["recipe"]
        self.assertEqual(recipe["environment"], {"LC_ALL": "C.UTF-8"})
        for case in matrix["cases"]:
            with self.subTest(case=case["id"]):
                source = (FIXTURES / case["file"]).read_bytes()
                self.assertEqual(hashlib.sha256(source).hexdigest(), case["sourceSha256"])
                self.assertEqual(source.count(b"BEFORE\n"), 1)
                self.assertEqual(source.count(b"AFTER\n"), 1)
                self.assertEqual(set(case["profiles"]), {"ascii", "utf8", "tree", "lint"})
                self.assertEqual(set(case["native"]["bodyRows"]), {"ascii", "utf8"})
                for profile, result in case["profiles"].items():
                    self.assertEqual(result["status"], 0)
                    self.assertEqual(result["argv"], recipe["profiles"][profile] + [case["file"]])
                    self.assertIn("-Ios=ManT", result["argv"])
                    self.assertRegex(result["stdoutSha256"], r"^[0-9a-f]{64}$")
                    self.assertRegex(result["stderrSha256"], r"^[0-9a-f]{64}$")

    def test_three_native_entry_paths_keep_exact_empty_row_counts(self):
        # Pinned man_term.c::print_man_node and mdoc_term.c::print_mdoc_node
        # dispatch empty no-fill TEXT to term_vspace. term.c preserves every
        # such asserted row; the reading exception below is deliberately separate.
        by_id = {case["id"]: case for case in cases()["cases"]}
        for kind in ["nf", "ex", "bd"]:
            for rows in [0, 1, 2]:
                case = by_id[f"{kind}_leading_{rows}"]
                with self.subTest(case=case["id"]):
                    expected = [""] * rows + ["LINE_A", "LINE_B"]
                    self.assertEqual(case["native"]["bodyRows"]["ascii"], expected)
                    self.assertEqual(case["native"]["bodyRows"]["utf8"], expected)
                    self.assertEqual(len(case["native"]["emptyNoFillTextSourceLines"]), rows)
                    self.assertEqual(case["readingException"]["dropLeadingEmptyRows"],
                                     1 if rows == 1 else 0)

    def test_all_empty_and_authored_whitespace_are_not_removal_candidates(self):
        # Native dispatch distinguishes an empty TEXT from nonempty whitespace
        # plus \&. Screen output alone cannot establish that source distinction.
        by_id = {case["id"]: case for case in cases()["cases"]}
        for identifier, rows in [("nf_empty_zero", 0), ("nf_empty_one", 1),
                                 ("ex_empty_two", 2), ("bd_empty_one", 1)]:
            case = by_id[identifier]
            self.assertEqual(case["native"]["bodyRows"]["utf8"], [""] * rows)
            self.assertTrue(case["sourceFacts"]["allEmpty"])
            self.assertEqual(case["readingException"]["rule"], "preserve")
        for identifier, whitespace in [("nf_authored_spaces", "spaces"),
                                      ("ex_authored_tab", "tab"),
                                      ("bd_authored_nbsp", "nbsp")]:
            case = by_id[identifier]
            self.assertEqual(case["sourceFacts"]["initialAuthoredWhitespace"], whitespace)
            self.assertEqual(case["native"]["emptyNoFillTextSourceLines"], [])
            self.assertEqual(case["readingException"]["dropLeadingEmptyRows"], 0)
        self.assertIn(b"\n  \\&\n", (FIXTURES / by_id["nf_authored_spaces"]["file"]).read_bytes())
        self.assertIn(b"\n\t\\&\n", (FIXTURES / by_id["ex_authored_tab"]["file"]).read_bytes())
        self.assertEqual(by_id["bd_authored_nbsp"]["native"]["bodyRows"]["utf8"][0], "\u00a0")

    def test_drains_tail_rows_and_navigation_have_independent_source_evidence(self):
        # Both native node drivers execute NODE_LINE before dispatching a font
        # request. Returning from a Rust output fragment is not another row event.
        by_id = {case["id"]: case for case in cases()["cases"]}
        case = by_id["nf_internal_and_trailing_rows"]
        self.assertEqual(case["native"]["bodyRows"]["utf8"],
                         ["", "LINE_A", "", "LINE_B", "", ""])
        self.assertEqual(case["readingException"]["dropLeadingEmptyRows"], 1)
        case = by_id["nf_controls_between_leading_rows"]
        self.assertEqual(case["native"]["bodyRows"]["utf8"],
                         ["", "", "LINE_A", "LINE_B"])
        self.assertEqual(case["native"]["emptyNoFillTextSourceLines"], [5, 7])
        self.assertEqual(case["readingException"]["rule"], "preserve")
        self.assertEqual(by_id["ex_link_after_leading_row"]["sourceFacts"]["linkTarget"],
                         "https://example.test/literal-rows")
        self.assertEqual(by_id["bd_anchor_after_leading_row"]["sourceFacts"]["anchor"],
                         "literal-anchor")
        self.assertEqual(by_id["nf_independent_literal_owners"]["readingException"]
                         ["dropLeadingEmptyRowsPerOwner"], [1, 1])
        for identifier in ["nf_control_after_leading_row", "bd_style_scope_after_leading_row",
                           "ex_link_after_leading_row", "bd_anchor_after_leading_row"]:
            self.assertEqual(by_id[identifier]["readingException"]["dropLeadingEmptyRows"], 1)


if __name__ == "__main__":
    unittest.main()
