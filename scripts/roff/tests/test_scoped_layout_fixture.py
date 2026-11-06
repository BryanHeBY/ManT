"""Check the finite native scoped-layout witnesses and page-wrapper removal."""

import hashlib
import json
import unittest

from scripts.roff.fixtures import record_scoped_layout_boundaries as recorder


class ScopedLayoutFixtureTests(unittest.TestCase):
    def test_exact_sources_and_profiles_stay_bound(self):
        fixture = json.loads(recorder.FIXTURE.read_text())
        self.assertEqual(fixture['case_count'], 16)
        self.assertEqual(fixture['profile_count'], 80)
        self.assertEqual(len(fixture['cases']), 16)
        self.assertEqual(len({case['id'] for case in fixture['cases']}), 16)
        for case in fixture['cases']:
            self.assertEqual(case['source_sha256'],
                             hashlib.sha256(case['source'].encode()).hexdigest())
            self.assertEqual(set(case['profiles']), set(recorder.PROFILES))
            self.assertEqual(case['ascii_rows'], case['utf8_rows'])
            for run in case['profiles'].values():
                self.assertEqual(run['status'], 0)
                self.assertEqual(len(run['stdout_sha256']), 64)
                self.assertEqual(run['stderr_sha256'], hashlib.sha256(b'').hexdigest())

    def test_native_rows_keep_scoped_origins_and_boundaries(self):
        fixture = json.loads(recorder.FIXTURE.read_text())
        cases = {case['id']: case['utf8_rows'] for case in fixture['cases']}
        self.assertEqual(cases['nested-tag-two-items'],
                         ['OUTER', '', '       #           First body.',
                          '', '       -           Second body.'])
        self.assertIn('            CODE', cases['tag-display-sibling'])
        self.assertEqual(cases['plain-display'], ['      CODE'])
        self.assertEqual(cases['negative-display-reading-origin'],
                         ['FIRST', '', '    CODE', '', 'SECOND', '      Second body.'])
        self.assertEqual(cases['nested-column-header'],
                         ['OUTER', '', '      Backend    Path                   Description',
                          '      uscsi      /dev/node              Body'])
        self.assertEqual(cases['nested-enum-paragraph'],
                         ['OUTER', '', '      1.   LAST', '', '      AFTER'])
        self.assertEqual(cases['nested-list-offset-origins'],
                         ['   OUTER', '', '               FIRST',
                          '                     First body.', '', '               SECOND',
                          '                     Second body.', '', '   LATER',
                          '         Later body.'])
        self.assertEqual(cases['ragged-display-paragraph'],
                         ['HEAD', '', '      .Ft', '      .Fc', '', 'AFTER'])
        self.assertEqual(cases['column-return-literal-origin'],
                         ['ENTRY', '', '              Key          Function',
                          '              q            Exit', '',
                          '        -h and -k are intended to ease use of the mode; for example:',
                          '', '              CODE'])
        self.assertEqual(cases['temporary-origin-with-author-blank'],
                         ['-r keyword', '        The default keyword for this option is:',
                          '             ipv4', '        AFTER'])
        self.assertEqual(cases['nested-item-paragraph'],
                         ['OUTER', '', '      LAST', '', '      AFTER'])

    def test_footer_removal_consumes_only_its_own_separator(self):
        output = ('DESCRIPTION\n     F\bFIRST  SECOND\n\n       TAIL\n\n\n'
                  'Native OS\n                               November 6, 2026'
                  '                           T(1)\n').encode()
        self.assertEqual(recorder.reading_rows(output),
                         ['FIRST  SECOND', '', '  TAIL', ''])
        single_footer = (b'DESCRIPTION\n     BODY\n\n'
                         b'OS                 November 6, 2026                 T(1)\n')
        self.assertEqual(recorder.reading_rows(single_footer), ['BODY'])
        with self.assertRaises(ValueError):
            recorder.reading_rows(b'DESCRIPTION\n    wrong-origin\n\n'
                                 b'OS November 6, 2026 T(1)\n')
        with self.assertRaises(ValueError):
            recorder.reading_rows(b'DESCRIPTION\n     BODY\n'
                                 b'OS November 6, 2026 T(1)\n')


if __name__ == '__main__':
    unittest.main()
