"""Fault-injection checks for the independent bounded geometry oracle."""
import unittest

from roff_layout_geometry import GeometryLimits, compare_layout_geometry, self_check


class GeometryTests(unittest.TestCase):
    def kinds(self, report):
        return {finding["kind"] for finding in report["findings"]}

    def test_missing_pp_gap_is_detected_even_with_single_character_rows(self):
        report = compare_layout_geometry("  A\n\n  B\n", "A\nB\n", ".nf\nA\n.Pp\nB\n.fi\n")
        self.assertIn("blank-gap", self.kinds(report))

    def test_actual_pp_sp_reference_gap_not_request_count_is_authority(self):
        source = ".Bd -literal -compact\nA\n.sp 1\n.Pp\n.Pp\nB\n.Ed\n"
        report = compare_layout_geometry("A\n\n\nB\n", "A\n\nB\n", source)
        gap, = [finding for finding in report["findings"] if finding["kind"] == "blank-gap"]
        self.assertEqual((gap["reference_blank_lines"], gap["mant_blank_lines"]), (2, 1))
        accepted = compare_layout_geometry("A\n\n\nB\n", "A\n\n\nB\n", source)
        self.assertFalse(accepted["findings"])

    def test_global_gutter_does_not_look_like_indentation_damage(self):
        source = ".nf\nA\n  B\nC\n.fi\n"
        report = compare_layout_geometry("       A\n         B\n       C\n", "A\n  B\nC\n", source)
        self.assertEqual(report["status"], "covered")
        self.assertFalse(report["findings"])

    def test_authored_indent_both_collapse_and_excess_are_candidates(self):
        source = ".nf\nA\n  B\nC\n.fi\n"
        for damaged in ["A\nB\nC\n", "A\n      B\nC\n"]:
            report = compare_layout_geometry("       A\n         B\n       C\n", damaged, source)
            self.assertIn("relative-origin", self.kinds(report))

    def test_native_nonbreaking_indent_has_real_terminal_width(self):
        source = ".SH TEST\n.nf\nBASE\n\\ \\ \\ \\ INDENTED\nTAIL\n.fi\n"
        reference = "TEST\n     BASE\n     \u00a0\u00a0\u00a0\u00a0INDENTED\n     TAIL\n"
        correct = "TEST\nBASE\n    INDENTED\nTAIL\n"
        self.assertEqual(compare_layout_geometry(reference, correct, source)['status'], 'covered')
        damaged = correct.replace('    INDENTED', 'INDENTED')
        self.assertIn('relative-origin', self.kinds(compare_layout_geometry(reference, damaged, source)))

    def test_table_cells_are_not_prose_anchors_but_following_damage_survives(self):
        source = ".SH TEST\n.TS\nl l.\nWORD\tCELLTWO\n.TE\n.nf\nAFTER\n.sp 1\nLAST\n.fi\n"
        reference = "TEST\nWORD   CELLTWO\nAFTER\n\nLAST\n"
        correct = "TEST\nWORD | CELLTWO\nAFTER\n\nLAST\n"
        result = compare_layout_geometry(reference, correct, source)
        self.assertEqual(result['status'], 'partial')
        self.assertFalse(result['findings'])
        self.assertIn('documented-table-column-geometry-difference', {r['reason'] for r in result['coverage']['uncovered']})
        damaged = correct.replace('AFTER\n\nLAST', 'AFTER\nLAST')
        self.assertIn('blank-gap', self.kinds(compare_layout_geometry(reference, damaged, source)))

    def test_explicit_rs_origin_is_relative_to_parent_not_global_mode(self):
        source = ".SH TEST\nOUTER\n.RS 4\n.nf\nA\nB\n.fi\n.RE\nAFTER\n"
        reference = "TEST\n       OUTER\n           A\n           B\n       AFTER\n"
        good = "TEST\nOUTER\n    A\n    B\nAFTER\n"
        self.assertFalse(compare_layout_geometry(reference, good, source)["findings"])
        bad = good.replace("    A", "        A").replace("    B", "        B")
        self.assertIn("relative-origin", self.kinds(compare_layout_geometry(reference, bad, source)))

    def test_nested_rs_owner_is_not_replaced_by_nearest_same_text(self):
        source = ".SH ONE\nOUTER\n.RS 4\n.nf\nFIRST\n.RS 2\nX\n.RE\nLAST\n.fi\n.RE\nEND\n"
        reference = "ONE\n OUTER\n     FIRST\n       X\n     LAST\n END\n"
        good = "ONE\nOUTER\n    FIRST\n      X\n    LAST\nEND\n"
        self.assertFalse(compare_layout_geometry(reference, good, source)["findings"])
        bad = good.replace("      X", "    X")
        report = compare_layout_geometry(reference, bad, source)
        self.assertIn("relative-origin", self.kinds(report))
        issue = next(f for f in report["findings"] if f["text"] == "x")
        self.assertEqual(len(issue["owner"]), 3)

    def test_filled_soft_wrap_is_not_a_source_hard_break(self):
        source = ".SH TEST\nalpha beta gamma\ndelta epsilon zeta\n.PP\nsecond paragraph words\n"
        reference = "TEST\n     alpha beta\n     gamma delta epsilon\n     zeta\n\n     second paragraph words\n"
        mant = "TEST\nalpha beta gamma delta epsilon zeta\n\nsecond paragraph words\n"
        report = compare_layout_geometry(reference, mant, source)
        self.assertEqual(report["status"], "covered")
        self.assertFalse(report["findings"])

    def test_repeated_rows_are_retained_as_occurrences(self):
        source = ".nf\nBEGIN\nX\n.Pp\nX\nEND\n.fi\n"
        reference = "BEGIN\nX\n\nX\nEND\n"
        report = compare_layout_geometry(reference, "BEGIN\nX\nX\nEND\n", source)
        self.assertIn("blank-gap", self.kinds(report))
        self.assertEqual(report["coverage"]["aligned_anchors"], 4)

    def test_repeat_moved_to_other_scope_is_not_satisfied_by_global_membership(self):
        source = ".SH ONE\n.nf\nA\nX\nB\n.fi\n.SH TWO\n.nf\nC\nX\nD\n.fi\n"
        reference = "ONE\nA\nX\nB\nTWO\nC\nX\nD\n"
        damaged = "ONE\nA\nB\nTWO\nC\nX\nX\nD\n"
        report = compare_layout_geometry(reference, damaged, source)
        self.assertIn("owner-position-or-occurrence-count", self.kinds(report))

    def test_unique_content_reordering_reports_owner_order(self):
        source = ".nf\nA\nB\nC\n.fi\n"
        report = compare_layout_geometry("A\nB\nC\n", "A\nC\nB\n", source)
        self.assertIn("source-owner-order", self.kinds(report))

    def test_literal_line_merge_is_distinct_from_zero_blank_gap(self):
        source = ".nf\nA\nB\n.fi\n"
        report = compare_layout_geometry("A\nB\n", "A B\n", source)
        self.assertIn("line-boundary", self.kinds(report))

    def test_font_state_is_transparent_but_unknown_glyphs_are_uncovered(self):
        good = compare_layout_geometry("A\nB\n", "A\nB\n", ".nf\n\\fBA\\fP\nB\n.fi\n")
        self.assertEqual(good["status"], "covered")
        unknown = compare_layout_geometry("α\nB\n", "α\nB\n", ".nf\n\\[alpha]\nB\n.fi\n")
        self.assertIn(unknown["status"], {"partial", "uncovered"})
        self.assertTrue(unknown["coverage"]["uncovered"])

    def test_backspace_movement_does_not_delete_an_unoverwritten_suffix(self):
        source = ".nf\nAB\nZ\n.fi\n"
        report = compare_layout_geometry("AB\nZ\n", "ABC\b\nZ\n", source)
        self.assertIn("missing-rendered-anchor", self.kinds(report))

    def test_dynamic_source_is_not_silently_called_clean(self):
        report = compare_layout_geometry("A\nB\n", "A\nB\n", ".de XX\nA\n..\n.XX\nB\n")
        self.assertEqual(report["status"], "uncovered")
        self.assertFalse(report["findings"])

    def test_documented_hp_and_device_layout_differences_are_explicitly_uncovered(self):
        for request in ["HP 4", "ce 2", "rj 2", "ti 4", "TS", "EQ"]:
            source = f".{request}\nA\n.br\nB\n"
            report = compare_layout_geometry("    A B\n", "A\nB\n", source)
            self.assertNotEqual(report["status"], "covered", request)
            self.assertTrue(report["coverage"]["uncovered"], request)

    def test_missing_origin_is_visible_even_when_inner_gaps_compare(self):
        source = ".Bd -literal -offset 4n\nA\nB\n.Ed\n"
        report = compare_layout_geometry("    A\n    B\n", "        A\n        B\n", source)
        self.assertEqual(report["status"], "partial")
        self.assertTrue(any(gap["reason"] == "parent-origin-not-observable" for gap in report["coverage"]["uncovered"]))

    def test_empty_no_evidence_and_budget_limits_never_report_covered(self):
        self.assertEqual(compare_layout_geometry("A", "A", None)["status"], "uncovered")
        self.assertEqual(compare_layout_geometry("", "", "")["status"], "uncovered")
        for limits in [GeometryLimits(source_bytes=2), GeometryLimits(anchors=1), GeometryLimits(rendered_tokens=1), GeometryLimits(alignment_work=1)]:
            result = compare_layout_geometry("A\nB\n", "A\nB\n", ".nf\nA\nB\n.fi\n", limits=limits)
            self.assertEqual(result["status"], "uncovered")
        with self.assertRaises(ValueError):
            compare_layout_geometry("", "", "", limits=GeometryLimits(anchors=0))

    def test_coverage_detail_and_repeated_occurrence_budgets_are_bounded(self):
        source = ".UNKNOWN\n" * 20
        report = compare_layout_geometry("", "", source, limits=GeometryLimits(coverage_details=2))
        self.assertEqual(len(report["coverage"]["uncovered"]), 2)
        self.assertGreater(report["coverage"]["uncovered_count"], 20)
        self.assertEqual(report["coverage"]["uncovered"][-1]["reason"], "coverage-details-truncated")
        report = compare_layout_geometry("X\nX\nX\n", "X\nX\nX\n", ".nf\nX\nX\nX\n", limits=GeometryLimits(occurrences_per_anchor=2))
        self.assertEqual(report["status"], "uncovered")
        self.assertEqual(report["coverage"]["aligned_anchors"], 0)


if __name__ == "__main__":
    self_check()
    unittest.main()
