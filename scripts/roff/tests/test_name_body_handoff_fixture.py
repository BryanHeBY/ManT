"""Finite N matrix axes and pristine source/profile identity stay bound."""
from collections import Counter
import json
import unittest

from scripts.roff.fixtures import name_body_handoff_cases as inputs
from scripts.roff.fixtures import record_name_body_handoffs as recorder


class NameBodyHandoffFixtureTests(unittest.TestCase):
    def test_source_identity_and_independent_expected_rows(self):
        fixture = json.loads(recorder.FIXTURE.read_text())
        self.assertEqual((fixture['case_count'], fixture['profile_count']), (45, 225))
        self.assertEqual(Counter(case['cohort'] for case in fixture['cases']),
                         {'core': 3, 'control': 14, 'head': 9, 'body': 7, 'state': 12})
        self.assertEqual(len({case['id'] for case in fixture['cases']}), 45)
        for source, recorded in zip(inputs.cases(), fixture['cases'], strict=True):
            with self.subTest(case=recorded['id']):
                for key, value in source.items():
                    self.assertEqual(recorded[key], value)
                oracle = recorded['oracle']
                self.assertEqual(recorder.digest(source['source']), oracle['source_sha256'])
                self.assertEqual(set(oracle['profiles']), set(recorder.PROFILES))
                for profile, receipt in oracle['profiles'].items():
                    self.assertEqual(receipt['status'],
                                     4 if source['context'] == 'table' and profile == 'lint' else 0)
                    self.assertRegex(receipt['stdout_sha256'], r'^[0-9a-f]{64}$')
                    self.assertRegex(receipt['stderr_sha256'], r'^[0-9a-f]{64}$')
        by_id = {case['id']: case for case in fixture['cases']}
        self.assertEqual(by_id['core-tag']['oracle']['rows'], ['key                   probe int'])
        self.assertEqual(by_id['core-hang']['oracle']['rows'], ['key                   probe int'])
        self.assertEqual(by_id['core-inset']['oracle']['rows'], ['key\u00a0probe', '      int'])


if __name__ == '__main__':
    unittest.main()
