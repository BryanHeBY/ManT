"""The publication matrix carries every source, explicit query and oracle receipt."""
from collections import Counter
from hashlib import sha256
from itertools import product
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/annotation_publication"
GENERATOR = ROOT / "scripts/roff/fixtures/annotation_publication_cases.py"


def expected_ids():
    """Enumerate legal axes independently of the source generator."""
    ids = set()

    def add(family, *axes):
        ids.update("/".join((family, *axis)) for axis in product(*axes))

    wrappers = ("UR", "MT")
    add("N1a", ("DESCRIPTION", "OPTIONS", "ENVIRONMENT"),
        ("digit", "flag", "command", "env"), ("xr", "ar", "lk", "mt", "sy"))
    add("N1b", ("empty", "provider", "both", "body", "none"),
        ("adjacent", "if0", "if1", "owntext"))
    add("N1c", ("mr", "tty", "ur", "none"), ("headless", "named", "intervening"))
    for wrapper in ("Lk", *wrappers):
        add("N2", (wrapper,), ("first", "prev"),
            ("none", "zero", "break", "font", "nbsp", "space", "recovery", "visible"),
            ("1n", "2n"), ("same", "split") if wrapper == "Lk" else ("same",))
    add("N3", (*wrappers, "plain"), ("first", "blank", "real"),
        ("one", "two", "zero", "twice", "br", "none"))
    add("N4", (*wrappers, "plain"), ("filled", "nofill"),
        ("EE", "EX", "fi", "nf", "br"), ("text", "bold", "zero", "continue"))
    for spelling in ("word", "BR", "IR"):
        add("E12", ("exact", "raw", "none", "near"), (spelling,),
            ("plain", *wrappers, "outside", "nofill", "code") if spelling == "word"
            else ("plain", *wrappers, "outside", "nofill"))
    add("E12extra", ("twice", "around", "lk", "tty"))
    add("E3", wrappers, ("eq", "first-eq", "eq-last", "first-eq-last", "config",
                         "config-word", "after", "closed"))
    add("X-head-marker", ("raw", "exact"), ("head", "body"))
    add("X-prefix-gap", wrappers, ("zero", "one", "two"))
    add("X-line-eqn", wrappers, ("EE", "fi", "nf", "PP"), ("last", "more"))
    return ids


def queries(case):
    family = case["family"]
    if family == "N1a":
        return [{"digit": "-1", "flag": "-f", "command": "run",
                 "env": "DEMO_HOME"}[case["axes"][1]]]
    return {
        "N1b": ["-a", "-b"], "N1c": ["--one"],
        "E12": ["printf"], "E12extra": ["printf"],
        "E3": ["TAIL"], "X-line-eqn": ["TAIL"],
        "X-head-marker": ["-a", "-b", "printf"],
    }.get(family, [])


class AnnotationPublicationFixtureTests(unittest.TestCase):
    def test_complete_ordered_sources_regenerate_in_empty_directories(self):
        recorded = (FIXTURE / "cases.jsonl").read_bytes()
        for _ in range(2):
            with tempfile.TemporaryDirectory() as scratch:
                generated = subprocess.check_output([sys.executable, str(GENERATOR)], cwd=scratch)
            self.assertEqual(generated.replace(b"\r\n", b"\n"), recorded)
        cases = [json.loads(row) for row in recorded.splitlines()]
        self.assertEqual(len(cases), 504)
        self.assertEqual({case["id"] for case in cases}, expected_ids())
        self.assertEqual(sum(len(case["queries"]) for case in cases), 224)
        for case in cases:
            self.assertEqual(case["id"], "/".join([case["family"], *case["axes"]]))
            self.assertEqual(queries(case), case["queries"], case["id"])
            self.assertTrue(case["source"].endswith("\n"), case["id"])
            self.assertEqual(sha256(case["source"].encode()).hexdigest(), case["sha256"])

    def test_every_source_has_five_fixed_reference_receipts(self):
        cases = [json.loads(row) for row in (FIXTURE / "cases.jsonl").read_bytes().splitlines()]
        native = [json.loads(row) for row in (FIXTURE / "native.jsonl").read_bytes().splitlines()]
        self.assertEqual(len(native), len(cases))
        counts = Counter()
        for case, receipt in zip(cases, native, strict=True):
            self.assertEqual(receipt["id"], case["id"])
            self.assertEqual(receipt["sha256"], case["sha256"])
            self.assertEqual(set(receipt["profiles"]), {"ascii", "utf8", "html", "tree", "lint"})
            for profile, (exit_code, stdout, stderr) in receipt["profiles"].items():
                counts[(profile, exit_code)] += 1
                self.assertRegex(stdout, r"\A[0-9a-f]{64}\Z")
                self.assertRegex(stderr, r"\A[0-9a-f]{64}\Z")
        self.assertEqual({key: count for key, count in counts.items() if key[0] != "lint"},
                         {("ascii", 0): 504, ("utf8", 0): 504, ("tree", 0): 504,
                          ("html", 0): 500, ("html", -11): 4})
        self.assertEqual({key: count for key, count in counts.items() if key[0] == "lint"},
                         {("lint", 0): 427, ("lint", 1): 56, ("lint", 2): 5, ("lint", 3): 16})

    def test_physical_row_expectations_keep_source_identity_and_review_reason(self):
        cases = [json.loads(row) for row in (FIXTURE / "cases.jsonl").read_bytes().splitlines()]
        selected = {case["id"]: case["sha256"] for case in cases
                    if case["family"] in {"N3", "N4", "X-prefix-gap"}}
        rows = [json.loads(row) for row in (FIXTURE / "rows.jsonl").read_bytes().splitlines()]
        self.assertEqual(len(rows), len(selected))
        self.assertEqual({row["caseId"]: row["sourceHash"] for row in rows}, selected)
        self.assertEqual(Counter(row["status"] for row in rows), {"Assert": 180})
        for row in rows:
            self.assertEqual(row["axis"], "physical-rows")
            self.assertEqual(row["consumer"], "cli-man")
            self.assertIn(row["ruleId"], {"N3-gap", "N4-row"})
            self.assertTrue(row["authority"].startswith("pinned ASCII and UTF-8"))
            self.assertTrue(row["expectation"][-1].endswith(("TAIL", "TAILMARK")))
            self.assertEqual(bool(row.get("reason")), row["status"] == "Review")


if __name__ == "__main__":
    unittest.main()
