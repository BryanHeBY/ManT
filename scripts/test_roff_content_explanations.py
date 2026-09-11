"""Adversarial occurrence and source-scope tests for content triage."""
from copy import deepcopy
import unittest
from unittest.mock import patch

from roff_content_compare import ContentLimits, compare_content
from roff_content_explanations import ExplanationLimits, assess_content, explain_content


SOURCE = '.Dd September 11, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd inspect text\n.Sh DESCRIPTION\nBODY – dash\n'
REFERENCE = 'NAME\nprobe – inspect text\nDESCRIPTION\nBODY – dash\n'
MANT = 'NAME\nprobe — inspect text\nDESCRIPTION\nBODY – dash\n'


class ExplanationTests(unittest.TestCase):
    def test_exact_source_bound_separator_preserves_raw_findings(self):
        result = explain_content(REFERENCE, MANT, SOURCE)
        self.assertEqual(result['status'], 'explained')
        self.assertEqual(result['rawComparison']['status'], 'review')
        self.assertEqual(result['residualComparison']['status'], 'covered')
        self.assertEqual(len(result['rawComparison']['findings']), 3)
        evidence = result['explanations'][0]
        self.assertEqual(evidence['sourceLine'], 6)
        self.assertEqual(evidence['referenceTokenRange'], [2, 3])
        self.assertEqual(evidence['mantTokenRange'], [2, 3])
        self.assertEqual(evidence['occurrences'], 1)

    def test_body_dash_change_cannot_be_explained(self):
        result = explain_content(REFERENCE, MANT.replace('BODY –', 'BODY —'), SOURCE)
        self.assertEqual(result['status'], 'review')
        self.assertEqual(len(result['explanations']), 1)
        self.assertEqual(result['residualComparison']['coverage']['missing_occurrences'], 1)
        self.assertEqual(result['residualComparison']['coverage']['extra_occurrences'], 1)

    def test_same_counts_but_swapped_occurrences_cannot_be_explained(self):
        wrong = REFERENCE.replace('BODY –', 'BODY —')
        result = explain_content(MANT, wrong, SOURCE)
        self.assertEqual(result['status'], 'review')
        self.assertEqual(result['explanations'], [])

    def test_missing_body_and_short_unicode_tokens_remain_review(self):
        for token in ['BODY', 'dash', '中', '7', '-x']:
            with self.subTest(token=token):
                ref = REFERENCE + token + '\n'
                result = explain_content(ref, MANT, SOURCE + token + '\n')
                self.assertEqual(result['status'], 'review')
                self.assertIn(token, [f.get('token') for f in result['residualComparison']['findings']])

    def test_authored_dash_in_Nd_line_must_match_exactly(self):
        source = SOURCE.replace('inspect text', 'inspect – text')
        ref = REFERENCE.replace('inspect text', 'inspect – text')
        mant = MANT.replace('inspect text', 'inspect — text')
        self.assertEqual(explain_content(ref, mant, source)['explanations'], [])

    def test_two_authored_description_dashes_do_not_borrow_generated_occurrence(self):
        source = SOURCE.replace('inspect text', 'inspect – text – again')
        ref = REFERENCE.replace('inspect text', 'inspect – text – again')
        mant = MANT.replace('inspect text', 'inspect – text — again')
        result = explain_content(ref, mant, source)
        self.assertEqual(result['status'], 'review')
        self.assertEqual(result['explanations'], [])

    def test_duplicate_NAME_text_elsewhere_is_not_an_anchor(self):
        result = explain_content('WRONG\n' + REFERENCE, MANT, SOURCE)
        self.assertEqual(result['explanations'], [])
        result = explain_content(REFERENCE * 2, MANT * 2, SOURCE)
        self.assertEqual(result['status'], 'review')
        self.assertEqual(len(result['explanations']), 1)

    def test_multiple_Nd_occurrences_only_first_is_explained(self):
        source = SOURCE.replace('.Sh DESCRIPTION', '.Nd another description\n.Sh DESCRIPTION')
        ref = REFERENCE.replace('DESCRIPTION\n', '– another description\nDESCRIPTION\n')
        mant = MANT.replace('DESCRIPTION\n', '— another description\nDESCRIPTION\n')
        result = explain_content(ref, mant, source)
        self.assertEqual(result['status'], 'review')
        self.assertEqual(len(result['explanations']), 1)
        self.assertEqual(result['residualComparison']['coverage']['missing_occurrences'], 1)

    def test_later_description_macros_do_not_expand_the_proof_scope(self):
        source = SOURCE.replace('.Sh DESCRIPTION', '.Em styled continuation\n.Sh DESCRIPTION')
        ref = REFERENCE.replace('DESCRIPTION\n', 'styled continuation\nDESCRIPTION\n')
        mant = MANT.replace('DESCRIPTION\n', 'styled continuation\nDESCRIPTION\n')
        self.assertEqual(explain_content(ref, mant, source)['status'], 'explained')
        result = explain_content(ref, mant.replace('styled continuation', 'styled'), source)
        self.assertEqual(result['status'], 'review')

    def test_Nm_names_and_comma_are_literal_not_guessed(self):
        source = SOURCE.replace('.Nm probe', '.Nm probe ,\n.Nm other')
        ref = REFERENCE.replace('probe –', 'probe, other –')
        mant = MANT.replace('probe —', 'probe, other —')
        self.assertEqual(explain_content(ref, mant, source)['status'], 'explained')
        self.assertEqual(explain_content(ref, mant.replace('other', 'wrong'), source)['explanations'], [])

    def test_argumentless_Nm_reuses_previously_proved_literal(self):
        source = SOURCE.replace('.Nm probe', '.Nm probe ,\n.Nm')
        ref = REFERENCE.replace('probe –', 'probe, probe –')
        mant = MANT.replace('probe —', 'probe, probe —')
        self.assertEqual(explain_content(ref, mant, source)['status'], 'explained')

    def test_Nd_is_not_inline_callable(self):
        source = SOURCE.replace('inspect text', 'Em inspect text')
        ref = REFERENCE.replace('inspect text', 'Em inspect text')
        mant = MANT.replace('inspect text', 'Em inspect text')
        self.assertEqual(explain_content(ref, mant, source)['status'], 'explained')

    def test_quoted_literals_and_soft_wrapping(self):
        source = SOURCE.replace('.Sh NAME', '.Sh "NAME"').replace('.Nd inspect text', '.Nd "inspect text"')
        self.assertEqual(explain_content(REFERENCE.replace('inspect text', 'inspect\n text'), MANT, source)['status'], 'explained')

    def test_apostrophe_is_literal_not_shell_quoting(self):
        source = SOURCE.replace('inspect text', "inspect user's text")
        ref = REFERENCE.replace('inspect text', "inspect user's text")
        mant = MANT.replace('inspect text', "inspect user's text")
        self.assertEqual(explain_content(ref, mant, source)['status'], 'explained')

    def test_NFC_and_known_bold_styling(self):
        source = SOURCE.replace('inspect text', 'inspect cafe\u0301')
        ref = REFERENCE.replace('probe –', 'p\bpr\bro\bob\bbe\be –').replace('inspect text', 'inspect café')
        mant = MANT.replace('inspect text', 'inspect cafe\u0301')
        self.assertEqual(explain_content(ref, mant, source)['status'], 'explained')

    def test_dynamic_prefixes_are_not_executed_or_guessed(self):
        for prefix in ['.if 1 .', '.de Nm\n..', '.am Nm\n..', '.als Nm Xr', '.so other', '.mso other', '.ig\n..', '.ec @', '.cc !', '.tr ab', '.ds x y', '.nr x 1']:
            with self.subTest(prefix=prefix):
                result = explain_content(REFERENCE, MANT, prefix + '\n' + SOURCE)
                self.assertEqual(result['status'], 'review')
                self.assertEqual(result['explanations'], [])

    def test_prefix_execution_boundary_cannot_be_crossed(self):
        for insertion in ['.if 1 .Nm probe', '.Pp', '.Sm off', '.Em extra', '.de x\n..', 'raw text']:
            with self.subTest(insertion=insertion):
                source = SOURCE.replace('.Nm probe', insertion + '\n.Nm probe')
                self.assertEqual(explain_content(REFERENCE, MANT, source)['explanations'], [])

    def test_escapes_unknown_quotes_empty_and_macro_names_are_not_guessed(self):
        for line in ['.Nd', '.Nd \\*[x]', '.Nd "inspect""text"', '.Nd "inspect', '.Nd inspect\\ text']:
            with self.subTest(line=line):
                source = SOURCE.replace('.Nd inspect text', line)
                self.assertEqual(explain_content(REFERENCE, MANT, source)['explanations'], [])
        for line in ['.Nm Em', '.Nm Bsx', '.Nm Brq', '.Nm', '.Nm probe other', '.Nm \\*[x]']:
            with self.subTest(line=line):
                self.assertEqual(explain_content(REFERENCE, MANT, SOURCE.replace('.Nm probe', line))['explanations'], [])

    def test_full_line_comments_do_not_execute_but_continuations_are_rejected(self):
        self.assertEqual(explain_content(REFERENCE, MANT, '.\\" copyright .de Nm\n' + SOURCE)['status'], 'explained')
        self.assertEqual(explain_content(REFERENCE, MANT, '.\\" continued\\\n' + SOURCE)['explanations'], [])

    def test_controls_cannot_be_explained(self):
        for marker in ['\0', '\x1b[2J', '\b', '\x85']:
            with self.subTest(marker=marker):
                result = explain_content(REFERENCE, MANT + marker, SOURCE)
                self.assertEqual(result['status'], 'hard-failure')
                self.assertEqual(result['explanations'], [])
        result = explain_content(REFERENCE + '\b', MANT, SOURCE)
        self.assertEqual(result['explanations'], [])

    def test_nonmdoc_and_absent_source_keep_review(self):
        for source in [None, SOURCE.replace('.Dt PROBE 1', '.TH PROBE 1'), SOURCE.replace('.Sh NAME', '.Sh DESCRIPTION')]:
            with self.subTest(source=source):
                self.assertEqual(explain_content(REFERENCE, MANT, source)['explanations'], [])

    def test_unframed_header_cannot_be_skipped(self):
        self.assertEqual(explain_content('PROBE(1)\n' + REFERENCE, 'probe(1)\n' + MANT, SOURCE)['explanations'], [])

    def test_name_or_description_case_is_not_folded(self):
        for wrong in [MANT.replace('probe', 'PROBE'), MANT.replace('inspect', 'Inspect')]:
            self.assertEqual(explain_content(REFERENCE, wrong, SOURCE)['explanations'], [])

    def test_unknown_bullet_and_quote_differences_stay_review(self):
        for ref, mant in [('NAME probe • inspect text', 'NAME probe - inspect text'), ('NAME probe “inspect” text', 'NAME probe "inspect" text')]:
            self.assertEqual(explain_content(ref, mant, SOURCE)['explanations'], [])

    def test_raw_evidence_can_be_reused_without_mutation_or_recomparison(self):
        raw = compare_content(REFERENCE, MANT, SOURCE)
        saved = deepcopy(raw)
        with patch('roff_content_explanations.compare_content', wraps=compare_content) as compare:
            result = explain_content(REFERENCE, MANT, SOURCE, raw_comparison=raw)
            self.assertEqual(compare.call_count, 1)  # Residual only.
        self.assertIs(result['rawComparison'], raw)
        self.assertEqual(raw, saved)

    def test_incomplete_raw_evidence_is_not_upgraded(self):
        result = explain_content(REFERENCE, MANT, SOURCE, limits=ContentLimits(max_findings=1))
        self.assertFalse(result['coverage']['comparisonComplete'])
        self.assertEqual(result['explanations'], [])

    def test_resource_limits_are_explicit(self):
        for limits in [ExplanationLimits(source_prefix_lines=2), ExplanationLimits(source_prefix_chars=10), ExplanationLimits(description_tokens=1)]:
            with self.subTest(limits=limits):
                result = explain_content(REFERENCE, MANT, SOURCE, explanation_limits=limits)
                self.assertEqual(result['explanations'], [])
                self.assertTrue(result['coverage']['reasons'])
        result = explain_content(REFERENCE, MANT, SOURCE, limits=ContentLimits(max_chars=10))
        self.assertEqual(result['status'], 'uncovered')
        with self.assertRaises(ValueError):
            ExplanationLimits(source_prefix_lines=0)

    def test_exact_documents_do_not_need_an_explanation(self):
        result = explain_content(MANT, MANT, SOURCE)
        self.assertEqual(result['status'], 'covered')
        self.assertEqual(result['explanations'], [])

    def test_terminal_hyphen_projection_is_source_consistent_and_never_mutates_raw_evidence(self):
        source = '.TH PROBE 1\nhttps://example.test/container-registry/path\n'
        reference = 'https://example.test/container-\n registry/path\n'
        mant = 'https://example.test/container-registry/path\n'
        result = assess_content(reference, mant, source)
        self.assertEqual(result['status'], 'explained')
        self.assertEqual(result['rawComparison']['status'], 'review')
        self.assertEqual(result['terminalPresentationComparison']['status'], 'covered')
        self.assertEqual(result['residualComparison']['status'], 'covered')
        self.assertEqual(result['explanations'][0]['rule'], 'cvs-terminal-breakable-hyphen/v1')
        self.assertTrue(result['coverage']['terminalHyphenSourceConsistent'])

    def test_terminal_hyphen_projection_requires_literal_source_occurrences(self):
        reference = 'NULL-\n terminated\n'
        mant = 'NULL-terminated\n'
        for source in (None, 'NULL-\nterminated\n', 'ordinary prose only\n'):
            with self.subTest(source=source):
                result = assess_content(reference, mant, source)
                self.assertEqual(result['status'], 'review')
                self.assertFalse(result['coverage']['terminalPresentationApplied'])
                self.assertEqual(result['rawComparison'], result['terminalPresentationComparison'])

    def test_terminal_hyphen_projection_requires_enough_repeated_source_occurrences(self):
        source = 'only one NULL-terminated spelling\n'
        reference = 'NULL-\n terminated\nNULL-\n terminated\n'
        mant = 'NULL-terminated\nNULL-terminated\n'
        result = assess_content(reference, mant, source)
        self.assertEqual(result['status'], 'review')
        self.assertFalse(result['coverage']['terminalPresentationApplied'])

    def test_terminal_hyphen_projection_keeps_nonrejoined_trailing_prose(self):
        source = 'compare-and-block trailing prose\n'
        reference = 'compare-and-\n block trailing prose\n'
        mant = 'compare-and-block trailing prose\n'
        result = assess_content(reference, mant, source)
        self.assertEqual(result['status'], 'explained')
        self.assertEqual(result['terminalPresentationComparison']['status'], 'covered')

    def test_direct_bullet_marker_is_a_source_consistent_compatibility_projection(self):
        source = '.IP \\(bu\nBODY\n'
        result = assess_content('• BODY\n', '- BODY\n', source)
        self.assertEqual(result['status'], 'explained')
        self.assertEqual(result['rawComparison']['status'], 'review')
        self.assertEqual(result['compatibilityPresentationComparison']['status'], 'covered')
        self.assertEqual(result['explanations'][0]['rule'], 'source-consistent-explicit-bullet-marker/v1')

    def test_direct_BR_manual_reference_spacing_is_a_source_consistent_projection(self):
        source = '.TS\nT{\n.BR accept (2)\nT}\n.TE\n'
        terminal_bold = ''.join(char + '\b' + char for char in 'accept')
        result = assess_content(terminal_bold + ' (2)\n', 'accept(2)\n', source)
        self.assertEqual(result['status'], 'explained')
        self.assertEqual(result['rawComparison']['status'], 'review')
        self.assertEqual(result['compatibilityPresentationComparison']['status'], 'covered')
        self.assertEqual(result['explanations'][0]['rule'], 'source-consistent-BR-manual-reference-spacing/v1')

    def test_literal_mdoc_column_table_separators_are_source_consistent_presentation(self):
        source = '''.Dd September 11, 2026
.Dt PROBE 1
.Os
.Bl -column "left" "right"
.It Sy "Left" Ta Sy "Right"
.It alpha Ta beta
.El
'''
        result = assess_content('Left Right\nalpha beta\n',
                                'Left | Right\nalpha | beta\n', source)
        self.assertEqual(result['status'], 'explained')
        self.assertEqual(result['rawComparison']['status'], 'review')
        self.assertEqual(result['compatibilityPresentationComparison']['status'], 'covered')
        self.assertEqual(result['explanations'][0]['rule'],
                         'source-consistent-mdoc-column-table-separators/v1')

    def test_column_separator_projection_permits_zero_width_no_call_escapes(self):
        source = '''.ds kept preamble
.Bl -column left right
.It "\\&left" Ta "\\&right"
.El
'''
        result = assess_content('left right\n', 'left | right\n', source)
        self.assertEqual(result['status'], 'explained')

    def test_column_separator_projection_refuses_ambiguous_or_incomplete_sources(self):
        valid = '''.Bl -column left right
.It left Ta right
.El
'''
        cases = [
            valid.replace('.It left Ta right', '.It left Ta right\\*[unsafe]'),
            valid.replace('.It left Ta right', '.It left "Ta" right'),
            valid.replace('.El', '.Bl -bullet\n.It nested\n.El\n.El'),
            valid.replace('.It left Ta right', '.if 1 .It left Ta right'),
            valid.replace('.El', '.El\ntext | literal'),
        ]
        for source in cases:
            with self.subTest(source=source):
                result = assess_content('left right\n', 'left | right\n', source)
                self.assertEqual(result['status'], 'review')
                self.assertFalse(result['coverage']['sourceConsistentCompatibilityApplied'])

    def test_column_separator_projection_requires_an_exact_output_inventory(self):
        source = '.Bl -column left right\n.It left Ta right\n.El\n'
        for mant in ('left | right | literal\n', 'left right\n'):
            with self.subTest(mant=mant):
                result = assess_content('left right\n', mant, source)
                self.assertNotEqual(result['status'], 'explained')
                self.assertFalse(result['coverage']['sourceConsistentCompatibilityApplied'])

    def test_compatibility_projection_requires_direct_source_evidence(self):
        for source, reference, mant in [
            ('plain prose\n', '• BODY\n', '- BODY\n'),
            ('.B accept (2)\n', 'accept (2)\n', 'accept(2)\n'),
            ('.BR accept (2)\n', 'accept (2)\naccept (2)\n', 'accept(2)\naccept(2)\n'),
        ]:
            with self.subTest(source=source):
                result = assess_content(reference, mant, source)
                self.assertEqual(result['status'], 'review')
                self.assertFalse(result['coverage']['sourceConsistentCompatibilityApplied'])

    def test_secondary_projection_can_reduce_a_capped_raw_finding_sample(self):
        source = '.IP \\(bu\nBODY\n'
        result = assess_content('• BODY\nALPHA\n', '- BODY\nBETA\n', source,
                                limits=ContentLimits(max_findings=1))
        self.assertFalse(result['rawComparison']['coverage']['complete'])
        self.assertIn('finding-retention-budget', result['rawComparison']['coverage']['reasons'])
        self.assertTrue(result['coverage']['sourceConsistentCompatibilityApplied'])
        self.assertLess(
            sum(result['compatibilityPresentationComparison']['counts'].values()),
            sum(result['rawComparison']['counts'].values()),
        )


if __name__ == '__main__':
    unittest.main()
