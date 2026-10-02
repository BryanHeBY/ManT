"""Check the finite Markdown core and independent oracle source bindings."""

import hashlib
import itertools
import json
from pathlib import Path
import unittest

from scripts.roff.fixtures.markdown_rule_cases import cases

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/markdown_hard_rows/cases.json"


class MarkdownRuleFixtures(unittest.TestCase):
    def test_all_container_carrier_and_hardline_tuples_are_frozen(self):
        frozen = json.loads(FIXTURE.read_text())
        generated = list(cases())
        self.assertEqual(len(generated), 100)
        self.assertEqual(len({case["id"] for case in generated}), 100)
        self.assertEqual(len({case["source"] for case in generated}), 100)
        expected = set(itertools.product(
            ["paragraph", "tag", "hang", "column", "literal"],
            ["No", "Em", "Sy", "Lk"],
            ["leading-one", "leading-two", "interior-empty", "trailing-one", "interior-one"]))
        actual = {(case["metadata"]["container"], case["metadata"]["carrier"],
                   case["metadata"]["hardline"]) for case in generated}
        self.assertEqual(actual, expected)
        for case, recorded in zip(generated, frozen["cases"], strict=True):
            for key, value in case.items():
                self.assertEqual(recorded[key], value)
            self.assertEqual(recorded["source_sha256"],
                             hashlib.sha256(recorded["source"].encode()).hexdigest())
            self.assertTrue(recorded["native_rows"])
            self.assertEqual(set(recorded["profiles"]), {"ascii", "utf8", "html", "tree", "lint"})
            for profile in recorded["profiles"].values():
                self.assertEqual(profile["status"], 0)
                self.assertEqual(len(profile["stdout_sha256"]), 64)
                self.assertEqual(len(profile["stderr_sha256"]), 64)

    def test_fence_consumers_do_not_claim_live_markdown_style_or_links(self):
        for case in cases():
            metadata = case["metadata"]
            self.assertTrue({"tui-buffer", "resize", "copy"}.issubset(metadata["consumers"]))
            self.assertIn("markdown-reader", metadata["expected_axes"])
            if metadata["container"] in {"literal", "column"}:
                self.assertEqual(set(metadata["uncovered"]),
                                 {"fenced-markdown-glyph-style", "fenced-markdown-active-links"})
            else:
                self.assertEqual(metadata["uncovered"], [])


if __name__ == "__main__":
    unittest.main()
