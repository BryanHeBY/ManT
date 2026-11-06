"""Native table execution gold keeps exact sources and independent receipts."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from scripts.roff.lib.roff_audit_common import discover_pages, manual_section
ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / 'crates/mant-engine/tests/roff_lowering/table_execution'

class TableExecutionFixtureTests(unittest.TestCase):
    def test_finite_domains_and_five_profile_native_gold(self):
        fixture = json.loads((FIXTURES / 'cases.json').read_bytes())
        self.assertEqual(len(fixture['sources']), 1045)
        self.assertEqual(len(fixture['cases']), 1188)
        counts = Counter(case['family'] for case in fixture['cases'])
        self.assertEqual((counts['T1'], counts['T2']), (192, 240))
        for digest, source in fixture['sources'].items():
            self.assertEqual(hashlib.sha256(source['source'].encode()).hexdigest(), digest)
            self.assertEqual(len(source['profiles']), 5)
            self.assertTrue(source['native_table_tree_rows'] or '.SY' in source['source'])
            self.assertIn(source['expected_label'], source['owner']['rows'][source['expected_label_owner_row']])
        for filename in ['adjacent.json', 'consumers.json', 'motion_controls.json']:
            cases = json.loads((FIXTURES / filename).read_bytes())['cases']
            for case in cases:
                self.assertEqual(hashlib.sha256(case['source'].encode()).hexdigest(), case['source_sha256'])
                self.assertEqual(set(case['profiles']), {'ascii','utf8','html','tree','lint'})

    def test_private_exhausted_budget_sources_reuse_complete_native_inputs(self):
        budget = ROOT / 'crates/mant-codec/src/mandoc/blocks/tables/recovery/budget_tests/fixtures/execution.json'
        cases = json.loads(budget.read_bytes())
        native = json.loads((FIXTURES / 'cases.json').read_bytes())['sources']
        self.assertEqual(len(cases), 16)
        for case in cases:
            digest = hashlib.sha256(case['source'].encode()).hexdigest()
            self.assertEqual(digest, case['source_sha256'])
            self.assertEqual(native[digest]['source'], case['source'])
            self.assertEqual(native[digest]['expected_label'], case['expected_label'])

    def test_section_zero_is_discovered_without_accepting_arbitrary_extensions(self):
        real = ROOT / 'tests/fixtures/roff/real/archlinux/termios.h.0p.gz'
        self.assertIn(real, discover_pages([real.parent]))
        self.assertEqual(manual_section(real), '0p')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in ['types.0p.gz', 'probe.1', 'probe.1m.zst', 'file.txt', 'README.md']:
                (root / name).touch()
            self.assertEqual({path.name for path in discover_pages([root])}, {'types.0p.gz','probe.1','probe.1m.zst'})

if __name__ == '__main__':
    unittest.main()
