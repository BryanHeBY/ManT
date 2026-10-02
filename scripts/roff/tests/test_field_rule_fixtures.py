"""Bound field inputs, actual AST admission and finite F coverage checks."""

import copy
import gzip
import itertools
import json
from pathlib import Path
import unittest

from scripts.roff.fixtures.field_projection_policies import (
    FIXTURE, digest, load_policies, qualified_policy, project_regions, tree_owners,
)
from scripts.roff.fixtures.rule_closure_fields import cases, WORDS, CONTROLS, WIDTHS
from scripts.roff.fixtures import record_field_supplements as supplements


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

    def test_native_hyphen_cell_is_not_an_ast_record_boundary(self):
        # This exact source ran all five pristine profiles before the owner
        # assertion. tree.c prints TEXT verbatim; the ASCII_HYPH native cell
        # in a-bc is not a new tree row and remains in the extended It HEAD.
        path = Path(__file__).resolve().parents[3] / (
            'tests/fixtures/roff/rule_boundaries/native_tree_witness.json')
        case = json.loads(path.read_text())
        binding = supplements.registered_binding()
        self.assertEqual(case['oracle_identity'], binding['identity'])
        self.assertEqual(case['reference_sha256'], binding['reference_sha256'])
        self.assertEqual(digest(case['source']), case['source_sha256'])
        self.assertEqual(set(case['profiles']), set(supplements.PROFILES))
        for profile in case['profiles'].values():
            self.assertEqual(profile['code'], 0)
            self.assertEqual(digest(profile['stdout']), profile['stdout_sha256'])
            self.assertEqual(profile['stderr'], '')
        tree = case['profiles']['tree']['stdout']
        raw = next(row for row in tree.split('\n') if '\x1c' in row)
        observed = [node for node in tree_owners(tree)
                    if '\x1c' in node['label']]
        self.assertEqual(len(observed), 1)
        self.assertEqual(observed[0]['owner'], 'It HEAD')
        self.assertIn(observed[0]['label'], raw)
        # A synthetic tree ancestry mutation moves the same TEXT to BODY.
        # The observer
        # must follow actual ancestry, even when its payload is identical.
        moved = tree.replace('It (head)', 'It (body)')
        moved_nodes = [node for node in tree_owners(moved)
                       if '\x1c' in node['label']]
        self.assertEqual(moved_nodes[0]['owner'], 'It BODY')

    def test_supplement_identities_sources_profiles_and_pending_cartesian_axes_are_bound(self):
        binding = supplements.registered_binding()
        for name, (count, unique) in supplements.FIXTURES.items():
            with self.subTest(fixture=name):
                frozen = json.loads((supplements.DIRECTORY / (name + '.json')).read_text())
                checked = supplements.validate_fixture(name, frozen, binding)
                self.assertEqual(len(checked), count)
                self.assertEqual(len({case['source_sha256'] for case in checked}), unique)

    def test_supplement_admission_hash_owner_and_format_mutations_are_rejected(self):
        # Each compact supplement has its own stored evidence format.
        for fixture_name in supplements.FIXTURES:
            baseline = json.loads((supplements.DIRECTORY / (fixture_name + '.json')).read_text())
            for mutation in ('raw-profile-format', 'source-hash', 'explicit-rows'):
                changed = copy.deepcopy(baseline)
                first = changed['cases'][0]
                if mutation == 'raw-profile-format':
                    if 'profiles' in first:
                        first['profiles']['lint']['stdout_sha256'] = 'bad'
                    else:
                        first['profile_sha256']['lint'] = 'bad'
                elif mutation == 'source-hash':
                    first['source_sha256'] = '0' * 64
                else:
                    first['expected_rows'] = ['two\nphysical rows']
                with self.subTest(fixture=fixture_name, mutation=mutation), self.assertRaises(ValueError):
                    supplements.validate_fixture(fixture_name, changed)
        for fixture_name, mutation in (('fixed_blank_rows', 'cell-coordinate'),
                                        ('head_word_padding_rows', 'word-assertion')):
            changed = json.loads((supplements.DIRECTORY / (fixture_name + '.json')).read_text())
            first = changed['cases'][0]
            if mutation == 'cell-coordinate':
                first['projection']['native_replacements'][0]['column'] = 10000
            else:
                first['expected_head_words'] += ' AUTHOR'
            with self.subTest(fixture=fixture_name, mutation=mutation), self.assertRaises(ValueError):
                supplements.validate_fixture(fixture_name, changed)
        name = 'pending_prefix_rows'
        frozen = json.loads((supplements.DIRECTORY / (name + '.json')).read_text())
        mutations = ('duplicate-id', 'source-hash', 'missing-profile', 'invalid-profile-hash',
                     'incomplete-profile', 'count', 'unique-count', 'oracle', 'oracle-identity',
                     'product-gold', 'unreachable-owner', 'source-coordinate', 'missing-owner',
                     'implicit-rows', 'Cartesian-axis', 'metadata-source', 'legal-as-recovery')
        for mutation in mutations:
            changed = copy.deepcopy(frozen)
            header, first = changed['header'], changed['cases'][0]
            if mutation == 'duplicate-id':
                changed['cases'][1]['id'] = first['id']
            elif mutation == 'source-hash':
                first['source_sha256'] = '0' * 64
            elif mutation == 'missing-profile':
                del first['profiles']['tree']
            elif mutation == 'invalid-profile-hash':
                first['profiles']['tree']['stdout_sha256'] = 'not-a-sha'
            elif mutation == 'incomplete-profile':
                first['profiles']['tree']['timeout'] = True
            elif mutation == 'count':
                header['count'] -= 1
            elif mutation == 'unique-count':
                header['unique_sources'] -= 1
            elif mutation == 'oracle':
                header['oracle_sha256'] = '0' * 64
            elif mutation == 'oracle-identity':
                header['oracle_identity'] = 'unregistered'
            elif mutation == 'product-gold':
                header['expectations_from_product'] = True
            elif mutation == 'unreachable-owner':
                first['native_owner']['reachable'] = False
            elif mutation == 'source-coordinate':
                first['native_owner']['after_owners'][0]['line'] = 1000
            elif mutation == 'missing-owner':
                first['native_owner']['after_owners'][0]['owner'] = None
            elif mutation == 'implicit-rows':
                first['expected_rows'] = 'X BodyWord'
            elif mutation == 'Cartesian-axis':
                first['pattern'] = changed['cases'][1]['pattern']
            elif mutation == 'legal-as-recovery':
                first['oracle_class'] = 'recovery'
            else:
                first['source'] = first['source'].replace('.It Xo\n', '.It Xo X\n')
                first['source_sha256'] = digest(first['source'])
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                supplements.validate_fixture(name, changed)

    def test_exact_tree_witness_rejects_row_owner_and_raw_binding_mutations(self):
        # The exact a-bc\\p \\p Q input ran pristine ASCII/UTF-8/HTML/tree/lint
        # before these assertions. tree.c prints native ASCII_HYPH inside TEXT;
        # term.c accepts the earlier field before rejecting its later suffix.
        path = Path(__file__).resolve().parents[3] / 'tests/fixtures/roff/rule_boundaries/native_tree_witness.json'
        witness = json.loads(path.read_text())
        raw = {name: dict(profile, stderr_sha256=digest(profile['stderr']),
                          utf8_valid=True, timeout=False)
               for name, profile in witness['profiles'].items()}
        case = dict(id='native-tree-witness', oracle_class='legal',
                    source=witness['source'], source_sha256=witness['source_sha256'],
                    profile_sha256={name: profile['stdout_sha256'] for name, profile in raw.items()},
                    expected_rows=['     X a-bc', '           BODY'],
                    head_word_source={'line': 11, 'column': 1}, body_word_source={'line': 13, 'column': 5})
        self.assertEqual(supplements.validate_live_case('witness', case, raw)['status'], 'verified')
        for mutation in ('raw-hash', 'owner', 'source-coordinate', 'row', 'timeout', 'legal-as-recovery'):
            changed, profiles = copy.deepcopy(case), copy.deepcopy(raw)
            if mutation == 'raw-hash':
                profiles['utf8']['stdout_sha256'] = '0' * 64
            elif mutation == 'owner':
                profiles['tree']['stdout'] = profiles['tree']['stdout'].replace('It (head)', 'It (body)')
                profiles['tree']['stdout_sha256'] = digest(profiles['tree']['stdout'])
                changed['profile_sha256']['tree'] = profiles['tree']['stdout_sha256']
            elif mutation == 'source-coordinate':
                changed['head_word_source']['column'] += 1
            elif mutation == 'row':
                changed['expected_rows'].append('')
            elif mutation == 'legal-as-recovery':
                changed['oracle_class'] = 'recovery'
            else:
                profiles['tree']['timeout'] = True
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                supplements.validate_live_case('witness', changed, profiles)

    def test_diagnosed_and_legal_admission_cannot_be_relabelled(self):
        # Pure status mutation: the formal verifier supplies actual statuses
        # from all five pristine runs before applying this shared mapping.
        profiles = {name: dict(code=0, utf8_valid=True, timeout=False)
                    for name in supplements.PROFILES}
        case = dict(id='admission-mutation', oracle_class='legal')
        supplements.validate_admission(case, profiles)
        with self.assertRaisesRegex(ValueError, 'admission class'):
            supplements.validate_admission(dict(case, oracle_class='recovery'), profiles)
        profiles['lint']['code'] = 2
        supplements.validate_admission(dict(case, oracle_class='diagnosed'), profiles)
        with self.assertRaisesRegex(ValueError, 'admission class'):
            supplements.validate_admission(case, profiles)
        profiles['lint']['code'] = True
        with self.assertRaisesRegex(ValueError, 'status format'):
            supplements.validate_admission(dict(case, oracle_class='diagnosed'), profiles)

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
