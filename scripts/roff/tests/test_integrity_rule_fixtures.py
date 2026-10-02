"""Keep omission depth, owner, ordinary-warning controls and gold bindings fixed."""

import itertools
import json
from pathlib import Path
import unittest

from scripts.roff.fixtures.integrity_rule_cases import cases
from scripts.roff.fixtures.replay_rule_boundaries import sha, PROFILES

FIXTURE = Path(__file__).resolve().parents[3] / 'crates/mant-engine/tests/escape_coverage/cases.json'


class IntegrityRuleFixtures(unittest.TestCase):
    def test_sources_depth_owner_and_profile_bindings_are_complete(self):
        generated, recorded = list(cases()), json.loads(FIXTURE.read_text())
        self.assertEqual(len(generated), 115)
        self.assertEqual(sum(row['metadata']['native_guard_expected'] for row in generated), 30)
        for case, frozen in zip(generated, recorded, strict=True):
            for key in ('id', 'source', 'metadata'):
                self.assertEqual(case[key], frozen[key])
            self.assertEqual(sha(case['source']), frozen['source_sha256'])
            self.assertEqual(set(frozen['profile_hashes']), set(PROFILES))
            self.assertEqual(frozen['native_region']['status'], 'asserted')
            self.assertEqual(len(frozen['oracle_sha256']), 64)
        actual = {(case['metadata']['depth'], case['metadata']['closed'],
                   case['metadata']['prefix'], case['metadata']['carrier'])
                  for case in generated if case['rule_id'] == 'D01'}
        self.assertEqual(actual, set(itertools.product((1, 2, 8, 64, 255, 256, 257, 300),
                         (False, True), ('A', 'αe\u0301'), ('No', 'Lk', 'TEXT'))))
        table = {(case['metadata']['depth'], case['metadata']['owner'])
                 for case in generated if case['rule_id'] == 'D04'}
        self.assertEqual(table, set(itertools.product((255, 256, 257, 300),
                         ('native-cell', 'font-cell', 'source-fragment'))))
        controls = [case for case in generated if case['rule_id'] == 'D03']
        self.assertEqual(len(controls), 7)
        self.assertTrue(all(not case['metadata']['native_guard_expected'] for case in controls))


if __name__ == '__main__':
    unittest.main()
