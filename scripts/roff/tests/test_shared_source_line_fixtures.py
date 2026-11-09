"""Verify shared source-line identities and both directions of row errors."""

from collections import Counter
import gzip
from itertools import product
import json
import unittest

from scripts.roff.fixtures import record_shared_source_lines as fixture


class SharedSourceLineFixture(unittest.TestCase):
    def test_deferred_word_source_and_oracle_receipts(self):
        path = fixture.FIXTURE.parents[1] / "deferred_word_spaces" / "cases.json.gz"
        recorded = json.loads(gzip.decompress(path.read_bytes()))
        self.assertEqual(recorded["oracle_sha256"], fixture.load()["oracle_sha256"])
        self.assertEqual(recorded["terminal_width"], 160)
        cases = recorded["cases"]
        self.assertEqual(Counter(case["group"] for case in cases), {
            "N1": 720, "N1/same-line": 12, "N1b/field": 24, "N1b/link": 12,
            "N1b/register": 15, "N1b/partial": 4, "N1b/word-boundaries": 756,
            "N1b/keep": 162,
        })
        self.assertEqual(len({case["sha256"] for case in cases}), 1692)
        self.assertEqual(len({(case["group"], case["id"]) for case in cases}), len(cases))
        allowed = 0
        for case in cases:
            self.assertEqual(fixture.digest(case["source"].encode()), case["sha256"], case["id"])
            self.assertEqual(set(case["profiles"]), set(fixture.PROFILES), case["id"])
            for profile, (status, stdout, stderr) in case["profiles"].items():
                self.assertRegex(stdout, r"^[0-9a-f]{64}$")
                self.assertRegex(stderr, r"^[0-9a-f]{64}$")
                if profile in {"ascii", "utf8", "tree"}:
                    self.assertEqual(status, 0, (case["id"], profile))
            if "native_rows" in case:
                allowed += 1
                self.assertEqual(case["group"], "N1b/field")
                self.assertTrue(case["id"].startswith(("inset/", "diag/")))
                self.assertEqual(case["rows"], [row.replace("\N{NO-BREAK SPACE}", " ")
                                                for row in case["native_rows"]])
        self.assertEqual(allowed, 9)

    def test_consumer_sources_keep_exact_bytes_and_all_profiles(self):
        directory = fixture.FIXTURE.parents[1] / "source_line_consumers"
        recorded = json.loads((directory / "receipts.json").read_bytes())
        self.assertEqual(recorded["oracle_sha256"], fixture.load()["oracle_sha256"])
        self.assertEqual(recorded["terminal_width"], 160)
        self.assertEqual(set(recorded["cases"]), {path.name for path in directory.glob("*.1")})
        self.assertEqual(len(recorded["cases"]), 16)
        for name, case in recorded["cases"].items():
            self.assertEqual(case["sha256"], fixture.digest((directory / name).read_bytes()), name)
            self.assertEqual(set(case["profiles"]), set(fixture.PROFILES), name)
            if name in {"tab.1", "link-tab.1"}:
                self.assertIn("tab in filled text", case["profiles"]["lint"]["diagnostic"])

    def test_source_and_oracle_receipts(self):
        recorded = fixture.load()
        self.assertEqual(recorded["oracle_sha256"],
                         "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e")
        cases = recorded["cases"]
        self.assertEqual(len({(case["group"], case["id"]) for case in cases}), len(cases))
        self.assertEqual(Counter(case["group"] for case in cases), {
            "M0": 768, "M1": 360, "M1/nested": 24,
            "M2/body": 480, "M2/links": 120, "M2/head": 192, "M2/ohang": 48,
            "M2/head-spelling": 480, "M2/seam": 8, "M2/nested": 16, "M3": 126,
            "M4": 432, "M4/general": 512, "M4/original": 336,
            "M4/historical": 24, "M4/expansion": 32,
            "M5/display": 2, "M5/heading": 2, "M5/table": 3,
            "M6/equation-seam": 8, "M7": 216,
        })
        m0 = product(("man", "mdoc"), ("fi", "nf"),
                     ("plain", "sp", "tab", "sp-zero", "tab-zero", "escaped-space", "zero-space", "nbsp"),
                     ("word", "continue", "break", "zero", "zero-x", "zero-cell"),
                     ("word", "br", "sp", "eof"))
        self.assertEqual({case["id"] for case in cases if case["group"] == "M0"},
                         {"-".join(axes) for axes in m0})
        for case in cases:
            self.assertEqual(fixture.digest(case["source"].encode()), case["sha256"], case["id"])
            self.assertEqual(set(case["profiles"]), set(fixture.PROFILES))
            for profile, (status, stdout, stderr) in case["profiles"].items():
                self.assertRegex(stdout, r"^[0-9a-f]{64}$")
                self.assertRegex(stderr, r"^[0-9a-f]{64}$")
                if profile in {"ascii", "utf8", "tree"}:
                    self.assertEqual(status, 0, (case["id"], profile))
            if "projection_rows" in case:
                self.assertEqual(case["group"], "M5/table")
                self.assertEqual(fixture.line_difference(case["native_rows"], case["projection_rows"]),
                                 "missing-line")
        self.assertEqual({case["id"] for case in cases if "projection_rows" in case},
                         {"textblock", "synthetic-style-fragment"})
        # Pristine HTML still has the separately repaired held-tag crash.
        failures = [case for case in cases if case["profiles"]["html"][0] != 0]
        self.assertEqual(Counter((case["group"], case["profiles"]["html"][0])
                                 for case in failures), {("M2/body", -11): 16})
        self.assertEqual(Counter(case["profiles"]["lint"][0] for case in cases),
                         {0: 1557, 1: 6, 2: 2577, 3: 48, 4: 1})

    def test_historical_classification_preserves_both_directions(self):
        cases = fixture.load()["cases"]
        fixed = Counter()
        for case in cases:
            if case["group"] == "M4/original" and "before_rows" in case:
                fixed[fixture.line_difference(case["native_rows"], case["before_rows"])] += 1
        self.assertEqual(fixed, {"extra-line": 36, "missing-line": 6})


if __name__ == "__main__":
    unittest.main()
