"""Check the finite margin-flush receipt and its exact native row projection."""

import hashlib
import json
import unittest

from scripts.roff.fixtures import record_margin_flush_boundaries as recorder


class MarginFlushFixtureTests(unittest.TestCase):
    def test_sources_and_all_five_profiles_stay_bound(self):
        fixture = json.loads(recorder.FIXTURE.read_text())
        self.assertEqual(fixture['case_count'], 19)
        self.assertEqual(fixture['profile_count'], 95)
        self.assertEqual(len(fixture['cases']), 19)
        self.assertEqual(sum('historical_id' in case for case in fixture['cases']), 9)
        for case in fixture['cases']:
            self.assertEqual(case['source_sha256'],
                             hashlib.sha256(case['source'].encode()).hexdigest())
            self.assertEqual(set(case['profiles']), set(recorder.PROFILES))
            for run in case['profiles'].values():
                self.assertEqual(run['status'], 0)
                self.assertEqual(len(run['stdout_sha256']), 64)
                self.assertEqual(run['stderr_sha256'], hashlib.sha256(b'').hexdigest())

    def test_common_origin_does_not_fold_words_or_empty_rows(self):
        output = 'DESCRIPTION\n     FIRST  SECOND\n\n        TAIL\n\nNEXT\n'.encode()
        self.assertEqual(recorder.reading_rows(output),
                         ['FIRST  SECOND', '', '   TAIL'])
        with self.assertRaises(ValueError):
            recorder.reading_rows(b'DESCRIPTION\n    wrong-origin\nNEXT\n')


if __name__ == '__main__':
    unittest.main()
