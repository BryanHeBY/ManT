"""Name-body row controls retain independent source, whitespace and receipts."""
import json
import unittest

from scripts.roff.fixtures import record_name_body_origins as recorder


class NameBodyOriginFixtureTests(unittest.TestCase):
    def test_two_sources_keep_exact_rows_and_seven_profile_identities(self):
        fixture = json.loads(recorder.FIXTURE.read_text())
        self.assertEqual((fixture['case_count'], fixture['profile_count']), (2, 14))
        profiles = {name for name, _, _ in recorder.PROFILES}
        for source, case in zip(recorder.sources(), fixture['cases'], strict=True):
            with self.subTest(case=case['id']):
                self.assertEqual((case['id'], case['source']), (source['id'], source['source']))
                self.assertEqual(case['source_sha256'], recorder.digest(source['source']))
                self.assertEqual(set(case['profiles']), profiles)
                for receipt in case['profiles'].values():
                    self.assertEqual(receipt['status'], 0)
                    self.assertRegex(receipt['stdout_sha256'], r'^[0-9a-f]{64}$')
                    self.assertRegex(receipt['stderr_sha256'], r'^[0-9a-f]{64}$')
                expected = ['probe int', '       leading']
                if case['id'] == 'two-leading-rows':
                    expected.append('       second')
                self.assertEqual(case['native_rows'], expected)
                for row, styles in zip(case['native_rows'], case['native_styles'], strict=True):
                    self.assertEqual(len(row), len(styles))


if __name__ == '__main__':
    unittest.main()
