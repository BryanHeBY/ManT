"""The operand matrix records native execution, not projected candidate text."""
import collections
import hashlib
import json
import unittest

from scripts.roff.fixtures import enclosure_operand_cases, record_enclosure_operands


class EnclosureOperandContracts(unittest.TestCase):
    def test_core_and_direct_operand_axes_bind_exact_sources_and_profiles(self):
        fixture = json.loads(record_enclosure_operands.FIXTURE.read_bytes())
        canonical = enclosure_operand_cases.cases()
        self.assertEqual(len(canonical), 192)
        self.assertEqual(len({case['source'] for case in canonical}), 185)
        self.assertEqual(collections.Counter(case['cohort'] for case in canonical),
                         {'cursor-core': 64, 'raw-operands': 54, 'risk': 50, 'controls': 24})
        self.assertEqual(fixture['profile_count'], 960)
        self.assertEqual(len({case['id'] for case in canonical}), 192)
        self.assertEqual(fixture['oracle_sha256'],
                         '9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e')
        for actual, source in zip(fixture['cases'], canonical, strict=True):
            with self.subTest(case=actual['id']):
                for key, value in source.items():
                    self.assertEqual(actual[key], value)
                self.assertEqual(actual['source_sha256'], hashlib.sha256(actual['source'].encode()).hexdigest())
                self.assertEqual(set(actual['profiles']), {'ascii', 'utf8', 'html', 'tree', 'lint'})
                for name, profile in actual['profiles'].items():
                    self.assertEqual(len(profile['stdout_sha256']), 64)
                    self.assertEqual(len(profile['stderr_sha256']), 64)
                    self.assertIn(profile['status'], [0, 2] if name == 'lint' else [0])
                self.assertEqual(len(actual['rows']), len(actual['styles']))
                for row, fonts in zip(actual['rows'], actual['styles'], strict=True):
                    self.assertEqual(len(row), len(fonts))

    def test_native_controls_keep_invisible_empty_zero_and_break_distinct(self):
        recorded = {case['id']: case for case in json.loads(record_enclosure_operands.FIXTURE.read_bytes())['cases']}
        # Exact native inputs ran before these permanent expected assertions.
        for name, expected in [
            ('cursor-eo-empty-on-plain-text', ['ORIGIN', ' SECOND', 'TAIL']),
            ('risk-ordinary-empty-plain', ['ORIGIN', 'SECOND', 'TAIL']),
            ('control-true-empty-literal', ['ORIGIN', 'SECOND', 'TAIL']),
            ('control-eo-control-literal', ['ORIGIN', '', 'SECOND', 'TAIL']),
            ('operand-break-opening-filled', ['ORIGIN', 'AB', 'SECOND', 'TAIL']),
            ('control-open-zero-off-filled', ['ORIGIN', 'ECOND', 'TAIL']),
            ('control-eo-on-off-filled', ['ORIGIN', ' SECOND', 'TAIL']),
            ('control-eo-repeated-off-filled', ['ORIGIN', 'SECOND', 'TAIL']),
            ('control-en-on-off-filled', ['ORIGIN', ' SECOND', 'TAIL']),
            ('control-en-repeated-off-filled', ['ORIGIN', 'SECOND', 'TAIL']),
            ('control-en-on-off-literal', ['ORIGIN', '', 'SECOND', 'TAIL']),
            ('operand-ordinary-punctuation-both-filled', ['ORIGIN', '@B@ SECOND', 'TAIL']),
            ('operand-ordinary-punctuation-both-literal', ['ORIGIN', '@B@', 'SECOND', 'TAIL']),
        ]:
            self.assertEqual(recorded[name]['rows'], expected)
        font = recorded['operand-font-opening-filled']
        for row, styles in zip(font['rows'], font['styles'], strict=True):
            for word in ('SECOND', 'TAIL'):
                if word in row:
                    start = row.index(word)
                    self.assertEqual(styles[start:start + len(word)], [1] * len(word))

    def test_one_common_origin_never_erases_a_later_word_cell(self):
        profiles = {'utf8': {'stdout': 'DESCRIPTION\n     ORIGIN\n      SECOND\n\nNEXT\n'}}
        self.assertEqual(record_enclosure_operands.rows_and_fonts(profiles)[0], ['ORIGIN', ' SECOND'])


if __name__ == '__main__':
    unittest.main()
