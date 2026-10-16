"""Source identity and independent coverage of the table layout matrix."""

import gzip
import hashlib
import json
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "tests/fixtures/roff/table_layout"


class TableFixtureContractTests(unittest.TestCase):
    def test_original_cases_bind_complete_sources_and_native_facts(self):
        matrix = json.loads((FIXTURES / "cases.json").read_text())
        cases = matrix["cases"]
        self.assertEqual(len(cases), 18)
        self.assertEqual(len({case["file"] for case in cases}), 18)
        self.assertEqual({case["file"] for case in cases},
                         {path.name for path in FIXTURES.glob("*.1")})
        self.assertEqual(set(matrix["reference"]["profiles"]),
                         {"ascii", "utf8", "html", "tree", "lint"})
        self.assertEqual(sum(case["oracleLintStatus"] == 0 for case in cases), 16)
        for case in cases:
            with self.subTest(case=case["file"]):
                source = (FIXTURES / case["file"]).read_bytes()
                self.assertEqual(hashlib.sha256(source).hexdigest(), case["sourceSha256"])
                self.assertEqual(len(re.findall(br"(?m)^\.TS$", source)), case["tables"])
                self.assertEqual(len(case["facts"]), case["tables"])
                self.assertTrue(case["markers"])
                self.assertEqual(set(case["profiles"]),
                                 {"ascii", "utf8", "html", "tree", "lint"})
                for profile in case["profiles"].values():
                    self.assertRegex(profile["stdoutProjectionSha256"], r"^[0-9a-f]{64}$")
                    self.assertRegex(profile["stderrSha256"], r"^[0-9a-f]{64}$")
                for fact in case["facts"]:
                    self.assertGreater(fact["columns"], 0)
                    self.assertLessEqual(set(fact["borders"]),
                                         {"outer", "innerRows", "innerColumns"})
                    self.assertLessEqual(set(fact["borders"].values()), {"single", "double"})
                    self.assertEqual(fact["rowCount"], sum(fact["rowKinds"].values()))

    def test_real_cases_bind_immutable_members_without_host_dependencies(self):
        matrix = json.loads((FIXTURES / "real_cases.json").read_text())
        cases = matrix["cases"]
        self.assertEqual(len(cases), 7)
        self.assertEqual(sum(case["tableCount"] for case in cases), 104)
        self.assertEqual(sum(len(case["samples"]) for case in cases), 12)
        self.assertIn("archlinux/awk.1p.gz", {case["file"] for case in cases})
        for case in cases:
            with self.subTest(case=case["file"]):
                path = ROOT / "tests/fixtures/roff/real" / case["file"]
                stored = path.read_bytes()
                self.assertEqual(hashlib.sha256(stored).hexdigest(), case["fixtureSha256"])
                if path.suffix == ".gz":
                    # gzip is in Python's standard library. Zstandard bytes
                    # retain their stored identity here; Rust's actual loader
                    # covers decoding without requiring a host executable.
                    self.assertEqual(hashlib.sha256(gzip.decompress(stored)).hexdigest(),
                                     case["sourceSha256"])
                for sample in case["samples"]:
                    self.assertLess(sample["index"], case["tableCount"])
                    self.assertGreater(sample["columns"], 0)
                    self.assertTrue(sample["markers"])


if __name__ == "__main__":
    unittest.main()
