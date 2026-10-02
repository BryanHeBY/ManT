"""Bound field inputs, actual AST admission and finite F coverage checks."""

import copy
import gzip
import itertools
import json
import unittest

from scripts.roff.fixtures.field_projection_policies import (
    FIXTURE, digest, load_policies, qualified_policy, project_regions,
)
from scripts.roff.fixtures.rule_closure_fields import cases, WORDS, CONTROLS, WIDTHS


def fixture():
    with gzip.open(FIXTURE, 'rt', encoding='utf-8') as source:
        header = json.loads(next(source))
        return header, list(map(json.loads, source))


class FieldRuleFixtures(unittest.TestCase):
    def test_every_source_identity_and_actual_owner_matches_frozen_pristine(self):
        header, frozen = fixture()
        generated = list(cases())
        self.assertEqual((header['count'], header['unique_sources']), (3906, 3890))
        self.assertFalse(header['expectations_from_product'])
        self.assertEqual(len(generated), header['count'])
        self.assertEqual(header['source_classes'], {'legal': 3477, 'diagnosed': 429})
        self.assertEqual(header['region_statuses'], {'asserted': 3906})
        for expected, actual in zip(generated, frozen, strict=True):
            self.assertEqual(expected['id'], actual['id'])
            self.assertEqual(expected['source'], actual['source'])
            self.assertEqual(expected['metadata'], actual['metadata'])
            self.assertEqual(digest(expected['source']), actual['source_sha256'])
            self.assertTrue(actual['native_owner']['reachable'], actual['id'])
            self.assertEqual(set(actual['profiles']), {'ascii', 'utf8', 'html', 'tree', 'lint'})
            self.assertEqual(actual['profiles']['utf8']['stdout_sha256'],
                             actual['projection']['native_utf8_sha256'])
            self.assertEqual(actual['profiles']['tree']['stdout_sha256'],
                             actual['projection']['native_tree_sha256'])
            if actual['family'] in ('field-pre-br-macros', 'field-pre-br-owners', 'field-pre-br-man-links'):
                self.assertTrue(actual['native_owner']['branch_nodes'], actual['id'])
            for owner in actual['native_owner']['after_owners']:
                if actual['metadata'].get('kind') == 'diag':
                    self.assertEqual(owner['owner'], 'It BODY')

    def test_core_and_supplement_pairs_cover_the_declared_cartesian_axes(self):
        generated = list(cases())
        core = [case for case in generated if case['family'] == 'field-pre-br-core']
        tuples = {tuple(case['metadata'][key] for key in ('kind', 'word', 'control', 'width'))
                  for case in core}
        self.assertEqual(tuples, set(itertools.product(('tag', 'hang', 'column'), WORDS, CONTROLS, WIDTHS)))
        pairs = [case for case in generated if case['family'] == 'field-pre-br-pairs']
        actual = {tuple(pair) for case in pairs for pair in case['metadata']['pair_keys']}
        axes = [['kind=' + value for value in ('tag', 'hang', 'column', 'ohang', 'inset', 'diag')],
                ['state=' + value for value in ('no-fill', 'closed-row', 'accepted-prefix')],
                ['carrier=' + value for value in ('No', 'Em', 'Lk')],
                ['control=' + value for value in ('br', 'nf-fi', 'ti', 'nf-word-fi')]]
        expected = {pair for left, right in itertools.combinations(axes, 2)
                    for pair in itertools.product(left, right)}
        self.assertEqual(actual, expected)
        triples = [case['metadata']['shared_state_triple'] for case in generated
                   if case['family'] == 'field-pre-br-triples']
        self.assertEqual(len(set(triples)), 6)
        self.assertIn('accepted-rejected-wrapper', triples)
        self.assertIn('pending-zero-generated-owner', triples)

    def test_presentation_cards_are_exact_source_and_oracle_bound(self):
        _, frozen = fixture()
        policies = load_policies()
        self.assertEqual(sum('margin_policy' in card for card in policies.values()), 289)
        self.assertEqual(sum(bool(card['native_replacements']) for card in policies.values()), 120)
        self.assertEqual(sum(bool(card['native_row_joins']) for card in policies.values()), 4)
        self.assertEqual(sum(bool(card['product_table_seams']) for card in policies.values()), 12)
        # This is a pure binding test, not new behavioral gold. Synthetic raw
        # streams exercise every guard independently of the pristine fixture.
        selected = [next(case for case in frozen if 'margin_policy' in case['projection'])]
        selected += [next(case for case in frozen if case['projection'][key]) for key in
                     ('native_replacements', 'native_row_joins', 'product_table_seams')]
        for case in selected:
            oracle = {'utf8': {'stdout': 'UTF8'}, 'tree': {'stdout': 'TREE'}}
            card = dict(case['projection'], native_utf8_sha256=digest('UTF8'), native_tree_sha256=digest('TREE'))
            registry = {case['id']: card}
            binding = dict(identity=card['oracle_identity'], reference_sha256=card['oracle_sha256'])
            self.assertEqual(qualified_policy(case, oracle, binding, registry), (card, None))
            mutations = []
            for suffix in ('.No AUTHOR|PIPE\n', r'.No AUTHOR\[u007C]PIPE' + '\n', '.No CHANGED\n'):
                changed = dict(case, source=case['source'] + suffix)
                mutations.append((changed, oracle, binding, registry))
            for profile in ('utf8', 'tree'):
                changed = copy.deepcopy(oracle)
                changed[profile]['stdout'] += 'CHANGED'
                mutations.append((case, changed, binding, registry))
            mutations.append((case, oracle, dict(binding, reference_sha256='0' * 64), registry))
            mutations.append((dict(case, id=case['id'] + '-different-owner'), oracle, binding, registry))
            for inputs in mutations:
                accepted, error = qualified_policy(*inputs)
                self.assertEqual(accepted, {})
                self.assertIsNotNone(error)

    def test_explicit_projection_coordinates_never_hide_neighbor_content(self):
        _, frozen = fixture()
        replacement = next(case for case in frozen if case['projection']['native_replacements'])
        native = replacement['native_region']
        qualified, _ = project_regions(native, native, replacement['projection'])
        self.assertEqual(qualified['rows'], replacement['expected_rows'])
        changed = copy.deepcopy(native)
        cell = replacement['projection']['native_replacements'][0]
        row = changed['rows'][cell['row']]
        changed['rows'][cell['row']] = row[:cell['column']] + 'A' + row[cell['column'] + 1:]
        with self.assertRaisesRegex(ValueError, 'separator coordinate changed'):
            project_regions(changed, native, replacement['projection'])
        seam = next(case for case in frozen if case['projection']['product_table_seams'])
        rows = list(seam['expected_rows'])
        index = seam['projection']['product_table_seams'][0]['row']
        rows[index] = 'INNER | CELL'
        _, projected = project_regions(seam['native_region'], dict(status='asserted', rows=rows), seam['projection'])
        self.assertEqual(projected['rows'][index], 'INNER CELL')
        for mutation in ('AUTHOR | CELL', 'INNER | AUTHOR', 'INNER | CELL | AUTHOR'):
            rows[index] = mutation
            with self.assertRaisesRegex(ValueError, 'separator owner changed'):
                project_regions(seam['native_region'], dict(status='asserted', rows=rows), seam['projection'])


if __name__ == '__main__':
    unittest.main()
