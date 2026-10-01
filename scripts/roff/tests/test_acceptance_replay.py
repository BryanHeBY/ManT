"""Source-bound acceptance axes, physical edges and admission regressions."""

import hashlib
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import subprocess

from scripts.roff.fixtures import acceptance_comparison as comparison
from scripts.roff.fixtures import acceptance_regions as regions
from scripts.roff.fixtures import replay_roff_acceptance as replay


FIXTURES = Path(__file__).with_name("fixtures") / "acceptance_regions.json"


def sha(value):
    return hashlib.sha256(value.encode()).hexdigest()


def asserted(rows):
    return {"status": "asserted", "rows": rows}


class AcceptanceRegionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixtures = {record["id"]: record for record in json.loads(FIXTURES.read_text())}

    def test_native_sections_preserve_all_completed_content_edges(self):
        # Exact sources ran with the registered pristine reference before
        # these assertions were written. Section pre and foot vspace rules:
        # man_term.c::pre_SH()/print_man_foot(), mdoc_term.c equivalents.
        for name, record in self.fixtures.items():
            with self.subTest(case=name):
                self.assertEqual(sha(record["source"]), record["source_sha256"])
                result = regions.select_native_region(record["oracle"]["utf8"]["stdout"],
                                                      record["oracle"]["tree"]["stdout"])
                self.assertEqual(result["status"], "asserted", result)
                self.assertEqual(result["rows"], record["expected_rows"])

    def test_five_diagnostic_cases_detect_dropped_or_added_edge_rows(self):
        for name in ("physical-row-handoffs-0097", "physical-row-handoffs-0107",
                     "physical-row-handoffs-0126", "physical-row-handoffs-0205",
                     "physical-row-handoffs-0207"):
            with self.subTest(case=name):
                correct = self.fixtures[name]["expected_rows"]
                wrong = correct + [""] if name.endswith("0097") else correct[:-1]
                if name.endswith("0126"):
                    wrong = correct[1:]
                report = comparison.compare_axes(asserted(correct), asserted(wrong))
                self.assertFalse(report["rows"])
                verdict, failures = comparison.verdict("native-diagnostics", report, {}, [])
                self.assertEqual(verdict, "fail")
                self.assertIn("rows", failures)

    def test_footer_metadata_supports_one_row_and_wrapped_layouts(self):
        # Pure framing mutation: moving the same authenticated OS/date/title
        # fields between device rows cannot change retained content edges.
        record = self.fixtures["physical-row-handoffs-0205"]
        tree = record["oracle"]["tree"]["stdout"]
        meta = regions.metadata(tree)
        suffix = [meta["os"], meta["date"] + " TEST(1)"]
        expected = ["", ""]
        for footer in (suffix, [" ".join(suffix)]):
            raw = "DESCRIPTION\n" + "\n" * 3 + "\n".join(footer) + "\n"
            result = regions.select_native_region(raw, tree)
            self.assertEqual(result["rows"], expected)
        malformed = "DESCRIPTION\n\nUNKNOWN FOOTER TEST(1)\n"
        self.assertEqual(regions.select_native_region(malformed, tree)["status"], "uncovered")

    def test_margin_decoration_does_not_delete_authored_bar_or_blank_row(self):
        record = self.fixtures["margin-authored-bar"]
        result = regions.select_native_region(record["oracle"]["utf8"]["stdout"],
                                              record["oracle"]["tree"]["stdout"])
        self.assertEqual(result["rows"], ["     AUTHORED |", "", "     AFTER"])
        wrong = asserted(["     AUTHORED", "", "     AFTER"])
        self.assertFalse(comparison.compare_axes(result, wrong)["content"])

    def test_margin_end_without_an_installed_character_keeps_native_rows_unchanged(self):
        # Exact complete sources ran all five pristine profiles first.
        # roff_term.c::roff_term_pre_mc(None) sets ENDMC; term.c::endline
        # prints no margin glyph unless an earlier mc child installed one.
        records = json.loads(FIXTURES.with_name("margin_controls.json").read_text())
        for record in records:
            with self.subTest(case=record["id"]):
                self.assertEqual(sha(record["source"]), record["source_sha256"])
                for profile in record["oracle"].values():
                    self.assertEqual(profile["code"], 0)
                    self.assertEqual(sha(profile["stdout"]), profile["stdout_sha256"])
                    self.assertEqual(sha(profile["stderr"]), profile["stderr_sha256"])
                native = record["oracle"]["utf8"]["stdout"]
                tree = record["oracle"]["tree"]["stdout"]
                original = regions.visible_text(native).splitlines()
                projected, margin = regions.margin_projection(original, tree, "NEXT")
                self.assertEqual(projected, original)
                selected = regions.select_native_region(native, tree)
                if "expected_rows" in record:
                    self.assertEqual(margin["status"], "not-applicable")
                    self.assertEqual(selected["status"], "asserted")
                    self.assertEqual(selected["rows"], record["expected_rows"])
                else:
                    self.assertEqual(margin["status"], "uncovered")
                    self.assertEqual(selected["status"], "uncovered")

    def test_product_region_uses_ir_spacing_and_does_not_trim_edges(self):
        bundle = {"document": {"sections": [
            {"heading": {"content": [{"type": "text", "value": "DESCRIPTION"}]}},
            {"heading": {"content": [{"type": "text", "value": "NEXT"}]},
             "spacingBeforeLines": 1}]}}
        raw = "DESCRIPTION\n\nAFTER\n\n\nNEXT\nEND\n"
        result = regions.select_product_region(raw, bundle)
        self.assertEqual(result["rows"], ["", "AFTER", ""])

    def test_ambiguous_content_heading_is_uncovered(self):
        record = self.fixtures["physical-row-handoffs-0126"]
        raw = record["oracle"]["utf8"]["stdout"].replace("     AFTER", "DESCRIPTION")
        self.assertEqual(regions.select_native_region(raw, record["oracle"]["tree"]["stdout"])
                         ["status"], "uncovered")

    def test_corrected_rs_templates_witness_a_real_native_body(self):
        cases = replay.all_cases()
        repaired = [case for case in cases if case["family"] == "corrected-owner-scopes"]
        self.assertEqual(len(repaired), 9)
        self.assertEqual(len(cases) - len(repaired), 8779)
        for case in repaired:
            with self.subTest(case=case["id"]):
                record = self.fixtures[case["id"]]
                self.assertEqual(case["source_sha256"], record["source_sha256"])
                oracle = record["oracle"]
                admission = replay.admit(case, {case["source_sha256"]: oracle})
                self.assertEqual(admission["admission"], "legal", admission)
                self.assertIn("RS (body)", oracle["tree"]["stdout"])
                broken = {**oracle, "tree": {**oracle["tree"],
                    "stdout": oracle["tree"]["stdout"].replace(
                        "RS (body)", "UNKNOWN (body)")}}
                self.assertEqual(replay.admit(case, {case["source_sha256"]: broken})
                                 ["admission"], "generator-defect")


class AcceptanceAxisTests(unittest.TestCase):
    def test_transport_records_timeout_without_abandoning_other_cases(self):
        with patch.object(replay.subprocess, "run", side_effect=subprocess.TimeoutExpired(
                ["candidate"], 1, output=b"PARTIAL", stderr=b"")):
            result = replay.run("candidate", [], "")
        self.assertTrue(result["timeout"])
        self.assertIsNone(result["code"])
        self.assertEqual(result["stdout"], "PARTIAL")

    def test_corrupt_internal_utf8_is_explicit_and_original_bytes_survive(self):
        process = subprocess.CompletedProcess(["candidate"], 0, b"A\xffB", b"")
        with patch.object(replay.subprocess, "run", return_value=process):
            result = replay.run("candidate", [], "")
        self.assertFalse(result["utf8_valid"])
        self.assertEqual(result["stdout_bytes_hex"], "41ff42")

    def test_responsive_padding_does_not_waive_word_or_hard_row_boundaries(self):
        policy = {"indent": "responsive", "padding": "responsive"}
        expected = asserted(["     D", "", "          AFTER BodyWord"])
        allowed = asserted(["D", "", "AFTER     BodyWord"])
        report = comparison.compare_axes(expected, allowed, policy)
        self.assertTrue(report["content"])
        self.assertTrue(report["rows"])
        self.assertTrue(report["separators"])
        joined = comparison.compare_axes(expected, asserted(["D", "", "AFTERBodyWord"]), policy)
        self.assertTrue(joined["content"])
        self.assertFalse(joined["separators"])
        merged = comparison.compare_axes(expected, asserted(["D AFTER BodyWord"]), policy)
        self.assertFalse(merged["rows"])

    def test_conservative_gap_policy_only_changes_the_declared_seam(self):
        policy = {"indent": "responsive", "padding": "responsive",
                  "conservative_gap": {"before": "BodyWord", "occurrence": 0,
                                       "occurrences": 1, "scalar_boundary": 6}}
        expected = asserted(["DAFTERBodyWord"])
        allowed = comparison.compare_axes(expected, asserted(["DAFTER    BodyWord"]), policy)
        self.assertTrue(allowed["separators"])
        wrong = comparison.compare_axes(expected, asserted(["D AFTER BodyWord"]), policy)
        self.assertFalse(wrong["separators"])

    def test_conservative_gap_permission_is_specific_to_one_occurrence(self):
        policy = {"indent": "responsive", "padding": "responsive",
                  "conservative_gap": {"before": "BodyWord", "occurrence": 1,
                                       "occurrences": 2, "scalar_boundary": 10}}
        native = asserted(["XBodyWord YBodyWord"])
        allowed = comparison.compare_axes(native, asserted(["XBodyWord Y BodyWord"]), policy)
        self.assertTrue(allowed["separators"])
        wrong = comparison.compare_axes(native, asserted(["X BodyWord Y BodyWord"]), policy)
        self.assertFalse(wrong["separators"])

    def test_nonbreaking_scalar_is_not_folded_into_an_ordinary_blank(self):
        report = comparison.compare_axes(asserted(["A\u00a0B"]), asserted(["A B"]),
                                         {"padding": "responsive"})
        self.assertFalse(report["content"])

    def test_accepted_literal_tab_cannot_be_replaced_by_an_ordinary_blank(self):
        # The source-bound Tab cards witness one source Tab between accepted
        # Y and Z; terminal expansion is separate from its literal scalar.
        policy = {"indent": "responsive", "padding": "responsive",
                  "tab": "device-expansion",
                  "literal_tab_sites": [{"row": 0, "scalar_boundary": 1}]}
        native = asserted(["     Y    Z", "     AFTER"])
        self.assertTrue(comparison.compare_axes(native, asserted(["Y\tZ", "AFTER"]),
                                               policy)["literal-tab-owner"])
        mutated = comparison.compare_axes(native, asserted(["Y Z", "AFTER"]), policy)
        self.assertTrue(mutated["content"])
        self.assertFalse(mutated["literal-tab-owner"])
        for wrong in (["Y Z", "\tAFTER"], ["YZ\t", "AFTER"], ["\tY Z", "AFTER"]):
            with self.subTest(rows=wrong):
                self.assertFalse(comparison.compare_axes(native, asserted(wrong), policy)
                                 ["literal-tab-owner"])

    def test_recovery_extent_is_asserted_despite_error_admission(self):
        # roff_escape.c uses the incomplete '[' escape's actual consumed end;
        # the frozen exact delimiter source has only AFTER in this region.
        report = comparison.compare_axes(asserted(["AFTER"]), asserted(["[a Z AFTER"]))
        status, failures = comparison.verdict("recovery", report, {}, [])
        self.assertEqual(status, "fail")
        self.assertIn("content", failures)

    def test_diagnostics_do_not_override_assertions_and_missing_axes_stay_review(self):
        report = comparison.compare_axes(asserted(["A B"]), asserted(["A B"]),
                                         {"indent": "omit-common-margin"})
        for admission in ("legal", "native-diagnostics", "recovery"):
            self.assertEqual(comparison.verdict(admission, report, {}, [])[0], "pass")
            self.assertEqual(comparison.verdict(admission, report, {}, ["source"])[0], "review")
        self.assertEqual(comparison.verdict("generator-scope-gap", report, {}, [])[0], "review")

    def test_source_or_oracle_change_revokes_display_policy(self):
        cards = comparison.load_policies()
        card = next(iter(cards.values()))
        # A different source cannot inherit permission through its case id.
        one = {"id": card["id"], "source_sha256": "different-source"}
        _, error = comparison.qualified_policy(one, {}, {}, cards)
        self.assertIn("source_sha256", error)
        self.assertIn("oracle_identity", error)
        self.assertIn("oracle_sha256", error)

    def test_external_identities_keep_authored_occurrence_order(self):
        target = {"kind": "external", "uri": "https://ex.org"}
        nodes = [{"type": "link", "target": target, "children": []},
                 {"type": "link", "target": target, "children": []}]
        bundle = {"document": {"sections": [{"blocks": [{"type": "paragraph", "children": nodes}]}]}}
        self.assertEqual(comparison.product_external_targets(bundle),
                         ["https://ex.org", "https://ex.org"])
        # Overstrike may affect HTML class spelling; href occurrence remains
        # authoritative for this narrow external-target axis.
        self.assertEqual(comparison.native_external_targets(
            '<a class="k" href="https://ex.org">A</a><a href="https://ex.org"></a>'),
            ["https://ex.org", "https://ex.org"])

    def test_email_identities_compare_the_same_href_without_losing_typed_occurrences(self):
        # All 16 wrapper-entries Mt complete sources ran the fixed pristine
        # five profiles first. mdoc_html.c::mdoc_mt_pre formats mailto:<raw>
        # before print_otag/print_encode. IR Email keeps just the address.
        record = json.loads(FIXTURES.with_name("email_identity.json").read_text())
        self.assertEqual(sha(record["source"]), record["source_sha256"])
        self.assertEqual(record["html"]["status"], 0)
        self.assertEqual(sha(record["html"]["stdout"]), record["html"]["stdout_sha256"])
        native = comparison.native_external_targets(record["html"]["stdout"])
        self.assertEqual(native, record["expected_targets"])

        def bundle(targets):
            return {"document": {"sections": [{"blocks": [{"type": "paragraph",
                "children": [{"type": "link", "target": target, "children": []}
                             for target in targets]}]}]}}

        email = {"kind": "email", "address": "x@example.org"}
        actual = comparison.product_external_targets(bundle([email]))
        self.assertEqual(actual, native)
        self.assertNotEqual(comparison.product_external_targets(bundle([])), native)
        self.assertNotEqual(comparison.product_external_targets(bundle([email, email])), native)
        wrong = {"kind": "email", "address": "wrong@example.org"}
        self.assertNotEqual(comparison.product_external_targets(bundle([wrong])), native)
        external = {"kind": "external", "uri": "https://ex.org"}
        self.assertEqual(comparison.product_external_targets(bundle([email, external, email])),
                         ["mailto:x@example.org", "https://ex.org", "mailto:x@example.org"])

    def test_exact_uri_width_cards_preserve_every_authored_head_and_word_boundary(self):
        # term.c::term_fill(vtarget) wraps only the target word at width78.
        # Exact source runs at 158/238/1000 agree before this assertion; the
        # preceding buffered \p HEAD rows remain distinct at every width.
        records = json.loads(FIXTURES.with_name("uri_widths.json").read_text())
        cards = comparison.load_policies()
        for record in records:
            with self.subTest(case=record["id"]):
                card = cards[record["id"]]
                self.assertEqual(sha(record["source"]), card["source_sha256"])
                self.assertEqual(sha(record["tree"]), card["native_tree_sha256"])
                self.assertEqual(record["widths"]["78"]["stdout_sha256"],
                                 card["native_utf8_sha256"])
                expected = card["expected_region"]
                for width, profile in record["widths"].items():
                    self.assertEqual(profile["status"], 0, width)
                    self.assertEqual(sha(profile["stdout"]), profile["stdout_sha256"], width)
                for width in ("158", "238", "1000"):
                    self.assertEqual(record["widths"][width]["region"], expected)
                actual = asserted([row[5:] if row.strip(" ") else row
                                   for row in expected["rows"]])
                report = comparison.compare_axes(expected, actual, card)
                self.assertTrue(report["content"])
                self.assertTrue(report["rows"])
                self.assertTrue(report["separators"])
                self.assertTrue(report["indent"])
                collapsed = asserted([" ".join(actual["rows"])])
                self.assertFalse(comparison.compare_axes(expected, collapsed, card)["rows"])
                wrong = asserted([row.replace("https://ex.orgAFTER", "https://ex.org AFTER")
                                  for row in actual["rows"]])
                self.assertFalse(comparison.compare_axes(expected, wrong, card)["separators"])


class AcceptanceBoundaryPolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.records = json.loads(FIXTURES.with_name("zero_advance_boundaries.json").read_text())
        cls.cards = comparison.load_policies()
        cls.cases = {case["id"]: case for case in replay.all_cases()}

    @staticmethod
    def binding(record):
        return {"identity": record["oracle_identity"],
                "reference_sha256": record["oracle_sha256"]}

    def test_exact_boundary_cards_keep_every_native_content_row(self):
        # All eighteen complete sources ran the registered five pristine
        # profiles first. ESCAPE_SKIPCHAR carries BACKAFTER into a generated
        # word (term.c:771-773,901-928): pre_SH/print_mdoc_foot then overwrite
        # its first scalar. Authenticate this exact raw scope, not a guessed
        # heading or host footer; the generic selector remains conservative.
        self.assertEqual(len(self.records), 18)
        for record in self.records:
            with self.subTest(case=record["id"]):
                case = self.cases[record["id"]]
                self.assertEqual(case["source"], record["source"])
                self.assertEqual(sha(record["source"]), record["source_sha256"])
                for name, profile in record["oracle"].items():
                    # post_section() may remove a trailing
                    # br/Pp and emit a warning; it never waives a row check
                    # (mdoc_validate.c:2666-2672).
                    self.assertIn(profile["code"], (0, 2) if name == "lint" else (0,))
                    self.assertEqual(sha(profile["stdout"]), profile["stdout_sha256"])
                    self.assertEqual(sha(profile["stderr"]), profile["stderr_sha256"])
                oracle = record["oracle"]
                raw, tree = oracle["utf8"]["stdout"], oracle["tree"]["stdout"]
                self.assertEqual(regions.select_native_region(raw, tree),
                                 record["unqualified_region"])
                card, error = comparison.qualified_policy(
                    case, oracle, self.binding(record), self.cards)
                self.assertIsNone(error)
                rows = regions.visible_text(raw).splitlines()
                scope = record["native_slice"]
                self.assertEqual(rows[scope["start"]:scope["end"]],
                                 card["expected_region"]["rows"])
                self.assertEqual(rows[scope["end"]], "")
                self.assertEqual(card["original_region_failure"], record["unqualified_region"])
                self.assertEqual(card["expected_region"], record["expected_region"])
                witness = record["boundary_witness"]
                if witness["kind"] == "section":
                    self.assertEqual(raw.splitlines()[witness["row"]], witness["raw_row"])
                    self.assertEqual(rows[witness["row"]], witness["visible_heading"])
                else:
                    self.assertEqual(regions.metadata(tree), witness["metadata"])
                    self.assertEqual(raw.splitlines()[witness["start"]:], witness["raw_rows"])

    def test_boundary_permission_expires_when_any_source_or_oracle_binding_changes(self):
        for record in self.records:
            case, oracle = self.cases[record["id"]], record["oracle"]
            binding = self.binding(record)
            changes = [
                ({**case, "source": case["source"] + "\n"}, oracle, binding, "complete_source"),
                ({**case, "source_sha256": "different"}, oracle, binding, "source_sha256"),
                (case, {**oracle, "utf8": {**oracle["utf8"],
                                          "stdout": oracle["utf8"]["stdout"] + "\n"}},
                 binding, "native_utf8_sha256"),
                (case, {**oracle, "tree": {**oracle["tree"],
                                          "stdout": oracle["tree"]["stdout"] + "\n"}},
                 binding, "native_tree_sha256"),
                (case, oracle, {**binding, "identity": "different"}, "oracle_identity"),
                (case, oracle, {**binding, "reference_sha256": "different"}, "oracle_sha256"),
            ]
            for changed_case, changed_oracle, changed_binding, axis in changes:
                with self.subTest(case=record["id"], binding=axis):
                    policy, error = comparison.qualified_policy(
                        changed_case, changed_oracle, changed_binding, self.cards)
                    self.assertEqual(policy, {})
                    self.assertIn(axis, error)
                    result = {"product_region": record["expected_region"],
                              "product_rows": record["expected_region"]["rows"],
                              "external_targets": [], "ansi_parity": True, "marker_leak": False}
                    ledger = replay.build_ledger([changed_case],
                        {changed_case["source_sha256"]: changed_oracle},
                        {changed_case["source_sha256"]: result}, {}, {}, changed_binding)
                    observed = ledger["cases"][record["id"]]
                    self.assertFalse(observed["axis_report"]["policy-binding"])
                    self.assertIn("policy-binding", observed["failing_axes"])
                    self.assertEqual(observed["status"], "fail")

    def test_scope_cards_assert_added_and_missing_eof_rows_in_the_real_ledger(self):
        for record in self.records:
            case, oracle = self.cases[record["id"]], record["oracle"]
            expected = record["expected_region"]["rows"]
            actual = [row[5:] if row.startswith("     ") else row for row in expected]
            variants = [("exact", actual), ("added-row", actual + [""])]
            if actual and actual[-1] == "":
                variants.append(("missing-row", actual[:-1]))
            for mutation, rows in variants:
                with self.subTest(case=record["id"], mutation=mutation):
                    result = {"product_region": asserted(rows), "product_rows": rows,
                              "external_targets": [], "ansi_parity": True, "marker_leak": False}
                    ledger = replay.build_ledger([case], {case["source_sha256"]: oracle},
                        {case["source_sha256"]: result}, {}, {}, self.binding(record))
                    observed = ledger["cases"][case["id"]]
                    self.assertEqual(observed["region"], record["unqualified_region"])
                    self.assertTrue(observed["axis_report"]["content"])
                    self.assertTrue(observed["axis_report"]["separators"])
                    if mutation == "exact":
                        self.assertTrue(observed["axis_report"]["rows"])
                        self.assertNotIn("rows", observed["uncovered_axes"])
                        # These cards never manufacture style/query/TUI coverage.
                        self.assertEqual(observed["status"], "review")
                    else:
                        self.assertFalse(observed["axis_report"]["rows"])
                        self.assertEqual(observed["status"], "fail")
                        self.assertIn("rows", observed["failing_axes"])


if __name__ == "__main__":
    unittest.main()
