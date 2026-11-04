"""The legacy synopsis fixture stays immutable and corrections remain bound."""
import json
import unittest

from scripts.roff.fixtures import record_synopsis_word_observations as recorder


class SynopsisWordObservationsTests(unittest.TestCase):
    def test_every_correction_binds_its_exact_old_source_and_all_old_receipts(self):
        legacy = json.loads(recorder.LEGACY.read_bytes())
        observed = json.loads(recorder.OBSERVATIONS.read_bytes())
        self.assertEqual(observed["original_fixture_sha256"],
                         recorder.digest(recorder.LEGACY.read_bytes()))
        self.assertEqual(observed["oracle_sha256"], legacy["oracle_sha256"])
        self.assertEqual({case["id"] for case in observed["cases"]}, set(recorder.IDENTIFIERS))
        self.assertEqual(len(observed["cases"]), 20)
        by_id = {case["id"]: case for case in legacy["cases"]}
        for case in observed["cases"]:
            with self.subTest(case=case["id"]):
                old = by_id[case["id"]]
                self.assertEqual(case["source"], old["source"])
                self.assertEqual(recorder.digest(case["source"]), old["source_sha256"])
                self.assertEqual(case["source_sha256"], old["source_sha256"])
                self.assertEqual(case["original_profiles"], old["oracle"])
                self.assertEqual(set(case["profiles"]), set(recorder.PROFILES))
                for name, receipt in case["profiles"].items():
                    self.assertEqual(receipt["status"], old["oracle"][name]["status"])
                    if name != "lint":
                        self.assertEqual(receipt, old["oracle"][name])
                self.assertEqual([row.strip(" ") for row in case["native_rows"]], old["rows"])
                self.assertEqual(case["native_rows"][-1], " SUFFIX")
                self.assertEqual(case["portable_owners"][-1]["rows"], [" SUFFIX"])
                self.assertEqual(case["portable_rows"][-1], " SUFFIX")

    def test_ui_observations_bind_the_same_full_sources_without_rewriting_old_rows(self):
        recorded = json.loads(recorder.OBSERVATIONS.read_bytes())
        ui = json.loads(recorder.UI_OBSERVATIONS.read_bytes())
        self.assertEqual(ui, recorder.ui_observations(recorded))
        self.assertEqual(ui["original_fixture_sha256"],
                         recorder.digest(recorder.UI_LEGACY.read_bytes()))
        self.assertEqual(len(ui["cases"]), 8)
        for case in ui["cases"]:
            self.assertEqual(recorder.digest(case["source"]), case["source_sha256"])
            self.assertEqual(case["reading_rows"][-1], " SUFFIX")
            self.assertEqual(case["portable_owner_texts"][-1], " SUFFIX")

    def test_row_observer_keeps_a_word_cell_beyond_the_shared_device_origin(self):
        raw = b"header\nSYNOPSIS\n     PREFIX\n     call(arg);\n      SUFFIX\n\nNEXT\n"
        self.assertEqual(recorder.native_rows(raw), ["PREFIX", "call(arg);", " SUFFIX"])
        self.assertNotEqual(recorder.native_rows(raw.replace(b"      SUFFIX", b"     SUFFIX")),
                            recorder.native_rows(raw))


if __name__ == "__main__":
    unittest.main()
