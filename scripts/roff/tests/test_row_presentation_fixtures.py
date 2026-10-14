"""Bind row-presentation expectations to their exact pristine sources."""

import hashlib
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[3]
ENGINE = ROOT / "crates/mant-engine/tests/roff_lowering/row_presentation/alignment.json"
UI = ROOT / "crates/mant-ui/src/document/tests/row_origins/fixtures/alignment.json"
TI = ROOT / "crates/mant-codec/src/mandoc/tests/temporary_indent/matrix.json"
SYNOPSIS = ROOT / "crates/mant-codec/src/mandoc/tests/synopsis_rows/matrix.json"
MACRO_PRE = ROOT / "crates/mant-engine/tests/roff_lowering/row_presentation/synopsis.json"
HANGING = ROOT / "crates/mant-engine/tests/roff_lowering/row_presentation/hanging.json"
ORACLE_SHA = "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def digest(value):
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


class RowPresentationFixtures(unittest.TestCase):
    def test_alignment_sources_and_raw_profiles_are_bound(self):
        fixture = read(ENGINE)
        self.assertEqual(fixture["header"]["count"], len(fixture["cases"]))
        self.assertFalse(fixture["header"]["expectations_from_product"])
        self.assertEqual(fixture["header"]["oracle_sha256"], ORACLE_SHA)
        ids = set()
        for case in fixture["cases"]:
            with self.subTest(case=case["id"]):
                self.assertNotIn(case["id"], ids)
                ids.add(case["id"])
                self.assertEqual(digest(case["source"]), case["source_sha256"])
                self.assertEqual(set(case["profiles"]), {"ascii-80", "utf8-40", "utf8-80", "html", "tree", "lint"})
                for profile in case["profiles"].values():
                    self.assertEqual(digest(profile["stdout"]), profile["stdout_sha256"])
                self.assertEqual(case["profiles"]["lint"]["stdout"], "")
                self.assertEqual(case["profiles"]["lint"]["stderr"], "")

    def test_ui_subset_is_identical_to_its_independent_source_profiles(self):
        originals = {case["id"]: case for case in read(ENGINE)["cases"]}
        fixture = read(UI)
        self.assertFalse(fixture["header"]["expectations_from_product"])
        self.assertEqual(fixture["header"]["count"], len(fixture["cases"]))
        for case in fixture["cases"]:
            self.assertEqual(case, originals[case["id"]])

    def test_temporary_indent_expectations_bind_complete_sources(self):
        fixture = read(TI)
        self.assertFalse(fixture["oracle"]["expectations_from_product"])
        self.assertEqual(fixture["oracle"]["sha256"], ORACLE_SHA)
        ids = set()
        for case in fixture["cases"] + fixture["structural_cases"]:
            self.assertNotIn(case["id"], ids)
            ids.add(case["id"])
            self.assertEqual(digest(case["source"]), case["source_sha256"])
            self.assertIn(".ti", case["source"])
            self.assertIn("PROBE_START", case["source"])
            self.assertIn("PROBE_END", case["source"])
            self.assertIsInstance(case["native_rows"], list)

    def test_synopsis_and_macro_pre_expectations_bind_complete_sources(self):
        for path, rows_key in [
            (SYNOPSIS, "native_rows"),
            (MACRO_PRE, "profiles"),
            (HANGING, "profiles"),
        ]:
            fixture = read(path)
            self.assertFalse(fixture["oracle"]["expectations_from_product"])
            self.assertEqual(fixture["oracle"]["sha256"], ORACLE_SHA)
            ids = set()
            for case in fixture["cases"]:
                with self.subTest(path=path.name, case=case["id"]):
                    self.assertNotIn(case["id"], ids)
                    ids.add(case["id"])
                    self.assertEqual(digest(case["source"]), case["source_sha256"])
                    self.assertTrue(case["source"].startswith((".TH PROBE", ".Dd ")))
                    self.assertEqual(set(case[rows_key]), {"ascii", "utf8"})
                    for rows in case[rows_key].values():
                        self.assertIsInstance(rows, list)
                        self.assertTrue(all(isinstance(row, str) for row in rows))


if __name__ == "__main__":
    unittest.main()
