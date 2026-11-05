"""Nm fixtures bind canonical full sources and exact five-profile observations."""
from collections import Counter
import itertools
import json
import unittest

from scripts.roff.fixtures import name_field_session_cases as inputs
from scripts.roff.fixtures import record_name_field_sessions as recorder


class NameFieldSessionFixtureTests(unittest.TestCase):
    def test_axes_source_identity_and_independent_diagnostics(self):
        fixture = json.loads(recorder.FIXTURE.read_text())
        cases = fixture['cases']
        self.assertEqual(Counter(case['cohort'] for case in cases),
                         {'core': 25, 'risk': 20, 'nested': 11})
        self.assertEqual(len({case['id'] for case in cases}), 56)
        self.assertEqual(fixture['profile_count'], 565)
        self.assertEqual(len(inputs.cases()), len(cases))
        for source, recorded in zip(inputs.cases(), cases):
            with self.subTest(case=recorded['id']):
                for key, value in source.items():
                    self.assertEqual(recorded[key], value)
                for role, body, native in [('whole', recorded['source'], recorded['whole']),
                                            ('fragment', recorded['independent_source'],
                                             recorded['independent'])]:
                    if body is None:
                        self.assertIsNone(native)
                        continue
                    self.assertEqual(recorder.digest(body), native['source_sha256'])
                    self.assertEqual(set(native['profiles']), set(recorder.PROFILES))
                    for profile, run in native['profiles'].items():
                        status = 4 if profile == 'lint' and role == 'whole' and recorded['independent'] else 0
                        if (recorded['id'], role, profile) == ('risk-prefix-Fn', 'fragment', 'html'):
                            status = -6
                            self.assertIn('html.c:650', run['diagnostics'])
                        self.assertEqual(run['status'], status)
                        self.assertRegex(run['stdout_sha256'], r'^[0-9a-f]{64}$')
                        self.assertRegex(run['stderr_sha256'], r'^[0-9a-f]{64}$')
        ids = {case['id'] for case in cases}
        for head, body in itertools.product(inputs.HEADS, inputs.BODIES):
            self.assertIn(f'core-{head}-{body}', ids)
        for index, context in itertools.product(range(4), ('head', 'Fo')):
            self.assertIn(f'nested-{context}-{index}', ids)
        by_id = {case['id']: case for case in cases}
        self.assertEqual(by_id['core-plain-Ft']['independent']['rows'], ['probe int'])
        self.assertEqual(by_id['risk-prefix-bare-zero']['independent']['rows'], ['robe  int'])
        self.assertEqual(by_id['risk-same-line-multi']['independent']['rows'], ['probe tail'])
        self.assertEqual(fixture['cell_control_count'], 4)
        self.assertEqual(len(fixture['cell_controls']), 4)
        for source, recorded in zip(inputs.cell_controls(), fixture['cell_controls']):
            for key, value in source.items():
                self.assertEqual(recorded[key], value)
            self.assertEqual(recorder.digest(recorded['source']), recorded['whole']['source_sha256'])
            for source, native in zip(recorded['fragments'], recorded['independent']):
                self.assertEqual(recorder.digest(source), native['source_sha256'])
                self.assertEqual(set(native['profiles']), set(recorder.PROFILES))
                self.assertTrue(all(run['status'] == 0 for run in native['profiles'].values()))

    def test_fixed_section_origin_never_consumes_an_authored_separator(self):
        raw = b'SYNOPSIS\n      FIRST\n     \n     SECOND  \n\nNEXT\n'
        rows, styles = recorder.projected_rows(raw)
        self.assertEqual(rows, [' FIRST', '', 'SECOND'])
        self.assertEqual([len(style) for style in styles], [6, 0, 6])


if __name__ == '__main__':
    unittest.main()
