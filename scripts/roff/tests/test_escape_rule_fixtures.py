"""Source/coverage checks; behavioral gold is always from pristine recording."""

import hashlib
import copy
import json
from itertools import product
from pathlib import Path
import unittest

from scripts.roff.fixtures.escape_rule_cases import cases
from scripts.roff.fixtures.escape_projection_policies import load_policies, qualified_policy, readable_recovery_rows

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-codec/src/mandoc/roff_escape/fixtures/escape_rules.json'


class EscapeRuleFixtures(unittest.TestCase):
    def test_frozen_sources_and_profile_hashes_match_the_generator(self):
        frozen = json.loads(FIXTURE.read_text())
        generated = cases()
        self.assertEqual((frozen['count'], frozen['unique_sources']), (2428, 2271))
        self.assertEqual(len(generated), frozen['count'])
        self.assertEqual(len({case['source'] for case in generated}), frozen['unique_sources'])
        for expected, actual in zip(generated, frozen['cases'], strict=True):
            self.assertEqual(expected['id'], actual['id'])
            self.assertEqual(expected['source'], actual['source'])
            self.assertEqual(hashlib.sha256(expected['source'].encode()).hexdigest(), actual['source_sha256'])
            self.assertEqual(actual['region_status'], 'asserted')
            self.assertEqual(set(actual['profiles']), {'ascii', 'utf8', 'html', 'tree', 'lint'})
            for profile in actual['profiles'].values():
                self.assertEqual(len(profile['stdout_sha256']), 64)
                self.assertEqual(len(profile['stderr_sha256']), 64)

    def test_standard_shapes_and_size_core_have_every_required_tuple(self):
        generated = cases()
        standard = {(case['metadata']['trigger'], case['metadata']['shape'],
                     case['metadata']['outer'], case['metadata']['position'], case['carrier'])
                    for case in generated if case['rule_id'] == 'E02'
                    and case['metadata'].get('shape') in {'single', 'two', 'bracket'}}
        self.assertEqual(standard, set(product('FMYkmf', ['single', 'two', 'bracket'],
                                               'oXZ', ['start', 'internal', 'end'], ['No', 'Lk'])))
        size = {(case['metadata']['sign'], case['metadata']['size'], case['metadata']['outer'],
                 case['metadata']['delimiter'], case['carrier'], case['metadata']['copy'])
                for case in generated if case['rule_id'] == 'E01' and 'delimiter' in case['metadata']
                and 'sign' in case['metadata']}
        self.assertEqual(size, set(product(['', '+', '-'],
                         ['1', '12', '24', '34', '40', '(12', '[12]', "'12'", "'1\\&2'"],
                         'oXZ', ["'", '|'], ['No', 'Lk'], [False, True])))

    def test_delimiter_branches_and_state_triples_are_explicit(self):
        generated = cases()
        delimiter = {(case['metadata']['outer'], case['metadata']['delimiter'],
                      case['metadata']['completion'], case['carrier'])
                     for case in generated if case['rule_id'] == 'E03'
                     and 'outer' in case['metadata']}
        self.assertEqual(delimiter, set(product('oXZChN',
                         ["'", '|', '\\(aq', '\\[aq]', '\\m[red]', '\\fB'],
                         ['closed', 'opening', 'unclosed'], ['No', 'Lk'])))
        state = {(case['metadata']['prefix'], case['metadata']['suffix'],
                  case['metadata']['outer'], case['carrier'])
                 for case in generated if case['rule_id'] == 'E05'}
        self.assertEqual(state, set(product(['', '\\z', '\\zP', 'A\\c', 'A\\p'],
                                            ['Z', 'line', 'link', 'owner'], 'oXZ', ['No', 'Lk'])))

    def test_missing_and_unclosed_postclassification_branches_are_frozen(self):
        generated = cases()
        size = {(case['metadata']['operand'], case['metadata']['prefix'], case['carrier'])
                for case in generated if case['rule_id'] == 'E01'
                and case['metadata'].get('branch') == 'mandatory-or-unclosed'}
        self.assertEqual(size, set(product(
            [sign + opening + payload for sign, opening, payload in product(
                ['', '+', '-'], ['', '(', '[', "'"], ['', '1'])],
            ['', '\\z'], ['No', 'TEXT'])))
        standard = {(case['metadata']['trigger'], case['metadata']['operand'],
                     case['metadata']['prefix'], case['carrier'])
                    for case in generated if case['metadata'].get('branch') == 'standard-eof-extent'}
        self.assertEqual(standard, set(product('FMYkmO',
            ['', '[', '(', '[B', '(B', '[]', '[B]'], ['', '\\z'], ['No', 'TEXT'])))
        quoted = {(case['metadata']['trigger'], case['metadata']['operand'],
                   case['metadata']['prefix'], case['carrier'])
                  for case in generated if case['rule_id'] == 'E03'
                  and case['metadata'].get('branch') == 'mandatory-or-unclosed'}
        self.assertEqual(quoted, set(product('DHLRSXZbvx',
            ['', "'", "'A", "'A'"], ['', '\\z'], ['No', 'TEXT'])))
        fonts = [case for case in json.loads(FIXTURE.read_text())['cases']
                 if case['metadata'].get('branch') == 'font-postclass']
        self.assertEqual(len(fonts), 81)
        self.assertTrue(all(len(case['expected_after_styles']) == 5 for case in fonts))
        extended = [case for case in fonts if case['metadata']['font_extension']]
        self.assertEqual(len(extended), 12)
        for case in extended:
            policy = case['style_policy']
            self.assertEqual(policy['rule'], 'documented-pandoc-verbatim-font')
            self.assertEqual(policy['source_sha256'], case['source_sha256'])
            self.assertEqual(policy['native_utf8_sha256'], case['profiles']['utf8']['stdout_sha256'])
            mask = {'C': 0, 'V': 0, '[VB]': 1, '[VI]': 2}[case['metadata']['operand']]
            self.assertEqual(policy['expected_product_after_styles'],
                             [mask if case['carrier'] == 'TEXT' else 0] * 5)
        for case in fonts:
            self.assertEqual('style_policy' in case, case['metadata']['font_extension'])

    def test_numbered_recovery_policies_require_exact_source_and_reference(self):
        policies = load_policies()
        self.assertEqual(len(policies), 24)
        for case in cases():
            if case['id'] not in policies:
                continue
            policy = policies[case['id']]
            oracle = policy['reference']
            binding = dict(identity=policy['oracle_identity'], reference_sha256=policy['oracle_sha256'])
            self.assertEqual(qualified_policy(case, oracle, binding, policy), (policy, None))
            self.assertEqual(readable_recovery_rows(case), policy['expected_product_readable_rows'])
            self.assertEqual(policy['uncovered'], ['style', 'identity', 'source', 'scalar-range', 'query', 'tui'])
            mutations = []
            for source in [case['source'] + '.No FORGED\n',
                           case['source'].replace('\\N', '\\C', 1),
                           case['source'].replace('"\n', "'\"\n", 1)]:
                changed = copy.deepcopy(case)
                changed['source'] = source
                mutations.append((changed, oracle, binding, policy))
            changed = copy.deepcopy(case)
            changed['metadata']['completion'] = 'closed'
            mutations.append((changed, oracle, binding, policy))
            for profile in ['utf8', 'tree']:
                changed = copy.deepcopy(oracle)
                changed[profile]['stdout'] += 'FORGED'
                mutations.append((case, changed, binding, policy))
            changed = dict(binding, reference_sha256='0' * 64)
            mutations.append((case, oracle, changed, policy))
            changed = copy.deepcopy(policy)
            changed['expected_product_readable_rows'][0] += 'FORGED'
            mutations.append((case, oracle, binding, changed))
            for inputs in mutations:
                accepted, error = qualified_policy(*inputs)
                self.assertIsNone(accepted)
                self.assertIn('binding changed', error)
        closed = [case for case in cases() if case['rule_id'] == 'E03'
                  and case['metadata'].get('outer') == 'N' and case['metadata']['completion'] == 'closed']
        self.assertEqual(len(closed), 12)
        for case in closed:
            self.assertIsNone(readable_recovery_rows(case))
            self.assertNotIn(case['id'], policies)


if __name__ == '__main__':
    unittest.main()
