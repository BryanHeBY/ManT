"""Immutable source and profile bindings of the live Rust consumer bridge."""

import hashlib
import json
from pathlib import Path
import unittest


FIXTURE = (Path(__file__).resolve().parents[3]
           / "crates/mant-engine/tests/roff_lowering/acceptance_axes/consumer_cases.json")


class ConsumerFixtureContractTests(unittest.TestCase):
    def test_every_selected_source_keeps_its_independent_five_profile_binding(self):
        # All exact sources ran pristine before the fixture and Rust behavior
        # assertions were written. This check never observes product output.
        fixture = json.loads(FIXTURE.read_text())
        self.assertEqual(fixture["oracle"]["sha256"],
                         "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6")
        self.assertEqual(len(fixture["cases"]), 34)
        names = set()
        for case in fixture["cases"]:
            with self.subTest(case=case["name"]):
                self.assertNotIn(case["name"], names)
                names.add(case["name"])
                self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                                 case["source_sha256"])
                self.assertEqual(set(case["profiles"]), {"ascii", "utf8", "html", "tree", "lint"})
                self.assertEqual(case["admission"], case["profiles"]["lint"]["status"])
                for name, profile in case["profiles"].items():
                    self.assertRegex(profile["stdout_sha256"], r"^[0-9a-f]{64}$")
                    self.assertRegex(profile["stderr_sha256"], r"^[0-9a-f]{64}$")
                    if name != "lint":
                        self.assertEqual(profile["status"], 0)
                self.assertEqual(case["rows"],
                                 [" ".join(cell for cell in row.split(" ") if cell)
                                  for row in case["native_rows"]])


if __name__ == "__main__":
    unittest.main()
