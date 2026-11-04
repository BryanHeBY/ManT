"""The recovery contract retains two separately authenticated source families."""

import collections
import hashlib
import json
import unittest

from scripts.roff.fixtures import record_source_sequences as recorder
from scripts.roff.fixtures import source_sequence_cases


class SourceSequenceContracts(unittest.TestCase):
    def test_canonical_source_inventory_and_independent_receipts(self):
        fixture = json.loads(recorder.FIXTURE.read_bytes())
        generated = source_sequence_cases.cases()
        self.assertEqual(len(generated), 96)
        self.assertEqual([case['id'] for case in fixture['cases']],
                         [case['id'] for case in generated])
        self.assertEqual(collections.Counter(case['cohort'] for case in generated),
                         {'core': 60, 'risk': 16, 'bibliography-clean': 6,
                          'bibliography-warning': 8, 'bibliography-clean-root': 6})
        for actual, canonical in zip(fixture['cases'], generated, strict=True):
            with self.subTest(case=actual['id']):
                for key, value in canonical.items():
                    self.assertEqual(actual[key], value)
                for source_key, role in [('source', 'whole'),
                                         ('independent_source', 'independent')]:
                    receipt = actual[role]
                    self.assertEqual(receipt['source_sha256'],
                                     hashlib.sha256(actual[source_key].encode()).hexdigest())
                    self.assertEqual(set(receipt['profiles']),
                                     {'ascii', 'utf8', 'html', 'tree', 'lint'})
                    for profile in receipt['profiles'].values():
                        self.assertEqual(len(profile['stdout_sha256']), 64)
                        self.assertEqual(len(profile['stderr_sha256']), 64)
                if actual['cohort'] == 'core':
                    self.assertEqual(actual['whole']['profiles']['lint']['status'], 4)
                    self.assertEqual(actual['independent']['profiles']['lint']['status'], 0)

    def test_include_owner_receipt_uses_executed_native_prefix(self):
        recorded = {case['id']: case for case in json.loads(recorder.FIXTURE.read_bytes())['cases']}
        self.assertEqual(sum(case['code_text'] is not None for case in recorded.values()), 12)
        # These exact native controls were recorded before this assertion.
        # The visible '<' can be consumed by bare zero-advance; an authored
        # static include spelling cannot stand in for the executed owner.
        for name, expected in [('word', ' <header.h>'),
                               ('bare-zero', ' header.h>'),
                               ('continue', '<header.h>')]:
            for first in ('Fa', 'No'):
                self.assertEqual(recorded[f'description-{first}-{name}-In']['code_text'], expected)
                self.assertEqual(recorded[f'synopsis-{first}-{name}-In']['code_text'],
                                 '#include <header.h>')

    def test_exact_expected_rows_do_not_collapse_empty_or_internal_space(self):
        profiles = {'utf8': {'stdout': 'SYNOPSIS\n     X  Y\n\n     Z\n\nNEXT\n'}}
        self.assertEqual(recorder.rows(profiles, 'SYNOPSIS'), ['X  Y', '', 'Z'])
        profiles['utf8']['stdout'] = 'SYNOPSIS\n\n     Z\n\nNEXT\n'
        self.assertEqual(recorder.rows(profiles, 'SYNOPSIS'), ['', 'Z'])

    def test_actual_source_line_receipts_keep_empty_words_and_continuation_distinct(self):
        recorded = json.loads(recorder.FIXTURE.read_bytes())['source_entries']
        canonical = source_sequence_cases.source_entries()
        self.assertEqual(len(recorded), 8)
        for actual, expected in zip(recorded, canonical, strict=True):
            with self.subTest(case=actual['id']):
                for key, value in expected.items():
                    self.assertEqual(actual[key], value)
                native = actual['native']
                self.assertEqual(native['source_sha256'],
                                 hashlib.sha256(actual['source'].encode()).hexdigest())
                self.assertEqual(set(native['profiles']),
                                 {'ascii', 'utf8', 'html', 'tree', 'lint'})
                self.assertTrue(all(profile['status'] == 0 for profile in native['profiles'].values()))
                expected_rows = (['CELL', 'A B'] if actual['id'].startswith('source-table-')
                                 else ['FIRSTSECOND'] if actual['id'].endswith('continued')
                                 else ['SECOND'])
                self.assertEqual(native['rows'], expected_rows)


if __name__ == '__main__':
    unittest.main()
