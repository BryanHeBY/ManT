"""Source-proved owner/occurrence regressions for the layout observation tool."""
import hashlib
import json
from pathlib import Path
import unittest

from scripts.roff.audit.audit_roff_fidelity import layout_comparison, layout_lines, no_fill_source_layout
from scripts.roff.lib.roff_line_observations import LineLimits, SourceRow, observe_hard_lines
from scripts.roff.lib import roff_line_observations

_FIXTURE = Path(__file__).with_name('fixtures') / 'line_observations.json'


class LineObservationTests(unittest.TestCase):
    def setUp(self):
        self.fixture = json.loads(_FIXTURE.read_text())
        self.cases = {case['id']: case for case in self.fixture['cases']}

    def observation(self, case, candidate):
        return layout_comparison(case['reference'], candidate, case['source'])

    def test_frozen_sources_and_pristine_observations_keep_their_identity(self):
        self.assertEqual(self.fixture['referenceSha256'], '482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6')
        for case in self.cases.values():
            self.assertEqual(hashlib.sha256(case['source'].encode()).hexdigest(), case['sourceSha256'])
            profiles = case['profiles']
            self.assertEqual(set(profiles), {'ascii', 'utf8', 'html', 'tree', 'lint'})
            record = profiles['utf8']
            self.assertEqual(record.get('code', record.get('exit')), 0)
            self.assertEqual(hashlib.sha256(case['reference'].encode()).hexdigest(), record['stdoutSha256'])

    def test_different_filled_source_split_cannot_claim_the_literal_occurrence(self):
        # Exact pristine runs preceded fixture/assertions. print_man_node applies
        # NODE_LINE in no-fill; filled source wraps do not create hard boundaries.
        case = self.cases['rclone-duplicate-flow-literal']
        report = self.observation(case, case['candidate'])
        self.assertEqual(report.hard_line_observation['status'], 'covered')
        self.assertEqual(report.hard_line_observation['comparedPairs'], 1)
        self.assertEqual(report.candidates, [])

    def test_missing_literal_boundary_is_not_hidden_by_a_correct_filled_copy(self):
        case = self.cases['rclone-duplicate-flow-literal']
        candidate = case['candidate'].replace('and\nthe default', 'and the default')
        report = self.observation(case, candidate).hard_line_observation
        self.assertEqual(report['mergedCount'], 1)
        self.assertEqual(report['merged'][0]['sourceLines'], [8, 9])
        self.assertNotEqual(report['merged'][0]['owner'], 0)

    def test_single_literal_boundary_corruption_has_exact_row_evidence(self):
        case = self.cases['rclone-single-literal']
        correct = self.observation(case, case['candidate']).hard_line_observation
        self.assertEqual(correct['status'], 'covered')
        candidate = case['candidate'].replace('and\nthe default', 'and the default')
        broken = self.observation(case, candidate).hard_line_observation
        self.assertEqual(broken['mergedCount'], 1)
        self.assertEqual(broken['merged'][0]['mantLines'][0], broken['merged'][0]['mantLines'][1])

    def test_two_identical_literal_payloads_still_have_distinct_owners(self):
        case = self.cases['duplicate-moved-owner']
        report = self.observation(case, case['reference']).hard_line_observation
        self.assertEqual(report['status'], 'covered')
        candidate = case['reference'].replace('Same first row\n', 'Same first row ', 1)
        report = self.observation(case, candidate).hard_line_observation
        self.assertEqual(report['mergedCount'], 1)
        self.assertEqual(report['merged'][0]['owner'], 3)

    def test_other_owner_cannot_replace_a_missing_literal_row(self):
        case = self.cases['duplicate-moved-owner']
        candidate = case['reference'].replace('Same second row\n', '', 1)
        candidate = candidate.replace('SecondEnd', 'Same second row\nSecondEnd')
        report = self.observation(case, candidate).hard_line_observation
        self.assertNotEqual(report['status'], 'covered')
        self.assertGreater(report['uncoveredCount'], 0)
        self.assertIn(3, {item.get('owner') for item in report['uncovered']})

    def test_actual_blank_rows_are_not_folded_by_token_alignment(self):
        case = self.cases['literal-blank-gap']
        correct = self.observation(case, case['reference'])
        self.assertEqual(correct.candidates, [])
        damaged = case['reference'].replace('First row\n\n', 'First row\n')
        report = self.observation(case, damaged)
        self.assertTrue(any('spacing divergence' in item for item in report.candidates))

    def test_repeated_literal_owners_cannot_erase_an_executed_blank_row(self):
        # Exact pristine five profiles ran first. Empty TEXT executes
        # man_term.c -> term_vspace(), independent of duplicate key spelling.
        case = self.cases['duplicate-owner-blank-gap']
        original = self.observation(case, case['reference']).hard_line_observation
        self.assertEqual(original['status'], 'covered')
        for occurrence in (1, 2):
            candidate = case['reference']
            if occurrence == 1:
                candidate = candidate.replace('Same first row\n\n', 'Same first row\n', 1)
            else:
                prefix, suffix = candidate.rsplit('Same first row\n\n', 1)
                candidate = prefix + 'Same first row\n' + suffix
            comparison = self.observation(case, candidate)
            report = comparison.hard_line_observation
            self.assertEqual(report['status'], 'review')
            self.assertEqual(report['comparedPairs'], 2)
            self.assertEqual(report['uncoveredCount'], 0)
            self.assertEqual(report['gapDifferenceCount'], 1)
            gap, = report['gapDifferences']
            self.assertEqual(gap['owner'], 3 if occurrence == 1 else 9)
            self.assertEqual((gap['referenceBlankLines'], gap['mantBlankLines']), (1, 0))
            self.assertTrue(any('spacing divergence' in value for value in comparison.candidates))

    def test_extra_literal_blank_rows_remain_owner_specific(self):
        case = self.cases['duplicate-owner-blank-gap']
        candidate = case['reference'].replace('Same first row\n\n', 'Same first row\n\n\n', 1)
        report = self.observation(case, candidate).hard_line_observation
        self.assertEqual(report['gapDifferenceCount'], 1)
        gap, = report['gapDifferences']
        self.assertEqual((gap['owner'], gap['referenceBlankLines'], gap['mantBlankLines']), (3, 1, 2))

    def test_source_continuation_is_not_invented_as_a_hard_boundary(self):
        case = self.cases['literal-continuation']
        report = self.observation(case, case['reference']).hard_line_observation
        self.assertNotEqual(report['status'], 'covered')
        self.assertEqual(report['mergedCount'], 0)

    def test_ambiguous_occurrence_counts_are_coverage_gaps(self):
        rows = [SourceRow(1, 1, 'duplicate row', True, 0), SourceRow(2, 1, 'another row', True, 0)]
        reference = layout_lines('duplicate row\nanother row\nduplicate row\nanother row\n')[0]
        report = observe_hard_lines(rows, reference, reference)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['comparedPairs'], 0)
        self.assertEqual(report['uncoveredCount'], 1)

    def test_each_budget_remains_explicit_and_does_not_grant_acceptance(self):
        source = '.nf\nfirst row\nsecond row\n.fi\n'
        rows = no_fill_source_layout(source).ordered_rows
        rendered = layout_lines('first row\nsecond row\n')[0]
        for limits in [LineLimits(source_rows=1), LineLimits(rendered_tokens=1), LineLimits(alignment_work=1)]:
            report = observe_hard_lines(rows, rendered, rendered, limits=limits)
            self.assertEqual(report['status'], 'partial')
            self.assertEqual(report['uncoveredCount'], 1)
        with self.assertRaises(ValueError):
            observe_hard_lines(rows, rendered, rendered, limits=LineLimits(details=0))

    def test_repeated_unresolved_landmarks_keep_bounded_work_and_details(self):
        rows = [SourceRow(i, i // 2, 'word', True, i // 2) for i in range(100)]
        rendered = layout_lines(('word\n' * 101))[0]
        report = observe_hard_lines(rows, rendered, rendered, limits=LineLimits(details=2))
        self.assertEqual(report['uncoveredCount'], 50)
        self.assertEqual(len(report['uncovered']), 2)
        self.assertLess(report['alignmentWork'], 50_000)

    def test_frequent_first_tokens_do_not_hide_a_unique_literal_boundary(self):
        # Exact source ran all five pristine profiles before these assertions.
        # print_man_node NODE_LINE and empty TEXT -> term_vspace preserve the
        # two physical rows and their blank gap. Anchor choice cannot waive it.
        case = self.cases['common-first-token']
        original = self.observation(case, case['reference']).hard_line_observation
        self.assertEqual((original['status'], original['comparedPairs']), ('covered', 1))
        self.assertLess(original['alignmentWork'], LineLimits().alignment_work)
        self.assertGreater(case['reference'].casefold().split().count('if'), 512)
        missing_gap = case['reference'].replace('If UniqueProbe row\n\n', 'If UniqueProbe row\n')
        gap = self.observation(case, missing_gap).hard_line_observation
        self.assertEqual((gap['gapDifferenceCount'], gap['uncoveredCount']), (1, 0))
        missing_boundary = case['reference'].replace('If UniqueProbe row\n\n', 'If UniqueProbe row ')
        merged = self.observation(case, missing_boundary).hard_line_observation
        self.assertEqual((merged['mergedCount'], merged['uncoveredCount']), (1, 0))

    def test_anchor_selection_preserves_exact_phrase_matches_in_the_old_covered_domain(self):
        limits = LineLimits()
        for case in self.cases.values():
            lines = layout_lines(case['reference'])[0]
            rendered = roff_line_observations._rendered(lines, limits)
            rows = no_fill_source_layout(case['source']).ordered_rows
            _, phrases, _ = roff_line_observations._groups(rows, limits)
            tokens, _, index = rendered
            for words in phrases:
                if len(index.get(words[0], ())) > limits.occurrences_per_row:
                    continue
                # The previous first-token candidate enumeration, limited to
                # its covered domain, is a separate exact match oracle.
                expected = [(start, start + len(words)) for start in index.get(words[0], ())
                            if tuple(tokens[start:start + len(words)]) == words]
                self.assertEqual(roff_line_observations._matches(words, rendered, limits, [0]), expected)

    def test_unmodeled_request_is_not_an_adjacency_proof(self):
        rows = no_fill_source_layout('.nf\nfirst row\n.Unknown visible\nsecond row\n.fi\n').ordered_rows
        self.assertNotEqual(rows[0].island, rows[1].island)
        rendered = layout_lines('first row second row\n')[0]
        self.assertEqual(observe_hard_lines(rows, rendered, rendered)['literalPairs'], 0)
        observation = layout_comparison('first row\nsecond row\n', 'first row second row\n', '.nf\nfirst row\n.Unknown visible\nsecond row\n.fi\n').hard_line_observation
        self.assertEqual(observation['status'], 'partial')
        self.assertEqual(observation['uncovered'][0]['sourceLine'], 3)


if __name__ == '__main__':
    unittest.main()
