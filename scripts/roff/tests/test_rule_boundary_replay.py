"""Registry, admission and mutation checks for the finite rule replay."""

import hashlib
import itertools
import json
from pathlib import Path
import subprocess
import tempfile
import copy
import unittest
from unittest.mock import patch

from scripts.roff.fixtures import acceptance_comparison as comparison
from scripts.roff.fixtures import replay_roff_acceptance as transport
from scripts.roff.fixtures import replay_rule_boundaries as replay


def asserted(rows):
    return {"status": "asserted", "rows": rows}


class RuleBoundaryRegistryTests(unittest.TestCase):
    def test_retained_review_sources_keep_the_independent_frozen_sequence(self):
        sources = [case["source"] for case in replay.assemble(["retained-review"])]
        self.assertEqual(len(sources), 3486)
        self.assertEqual(len(set(sources)), 3312)
        digest = hashlib.sha256(json.dumps(sources, ensure_ascii=False).encode()).hexdigest()
        self.assertEqual(digest, "75dd66232484ff012c86eb7934d1ca89fca6c6d5a16f334e89319eb7e59e328a")

    def test_core_axes_and_declared_pairs_are_cartesian_not_sample_counts(self):
        fields = replay.assemble(["field"])
        core = [case for case in fields if case["family"] == "field-pre-br-core"]
        self.assertEqual(len(core), 2880)
        actual = {tuple(case["metadata"][key] for key in ("kind", "word", "control", "width"))
                  for case in core}
        expected = set(itertools.product(("tag", "hang", "column"),
            replay.rule_closure_fields.WORDS, replay.rule_closure_fields.CONTROLS,
            replay.rule_closure_fields.WIDTHS))
        self.assertEqual(actual, expected)
        pair_cases = [case for case in fields if "pair_keys" in case["metadata"]]
        actual_pairs = {tuple(pair) for case in pair_cases for pair in case["metadata"]["pair_keys"]}
        axes = [["kind=" + value for value in ("tag", "hang", "column", "ohang", "inset", "diag")],
                ["state=" + value for value in ("no-fill", "closed-row", "accepted-prefix")],
                ["carrier=" + value for value in ("No", "Em", "Lk")],
                ["control=" + value for value in ("br", "nf-fi", "ti", "nf-word-fi")]]
        expected_pairs = {pair for left, right in itertools.combinations(axes, 2)
                          for pair in itertools.product(left, right)}
        self.assertEqual(actual_pairs, expected_pairs)

    def test_aliases_are_counted_as_identities_and_never_as_extra_sources(self):
        cases = replay.assemble(["markdown", "retained-review"])
        plan = replay.generation_plan(cases)
        self.assertEqual(plan["identities"], 3586)
        self.assertEqual(plan["unique_sources"], 3312)
        self.assertEqual(len({case["id"] for case in cases}), len(cases))

    def test_duplicate_identity_and_wrong_source_hash_are_rejected(self):
        case = {"id": "same", "source": "input", "family": "fixture"}
        with patch.object(replay.markdown_rule_cases, "cases", return_value=[case, case]):
            with self.assertRaisesRegex(ValueError, "duplicate execution identity"):
                replay.assemble(["markdown"])
        with patch.object(replay.markdown_rule_cases, "cases", return_value=[dict(case, source_sha256="wrong")]):
            with self.assertRaisesRegex(ValueError, "source hash changed"):
                replay.assemble(["markdown"])


class RuleBoundaryMutationTests(unittest.TestCase):
    @staticmethod
    def evaluated_case():
        one = replay.assemble(["integrity"])[-1]
        raw = {"code": 0, "stdout": "A\nAFTER\n", "stderr": "",
               "timeout": False, "utf8_valid": True,
               "stdout_sha256": replay.sha("A\nAFTER\n"), "stderr_sha256": replay.sha("")}
        oracle = dict(source_sha256=one["source_sha256"], **{name: dict(raw) for name in replay.PROFILES})
        product = {"region": asserted(["A", "AFTER"]), "targets": [],
                   "coverage_diagnostics": [{"impact": "content-coverage"}], "product_error": False}
        return one, oracle, product

    def test_cached_raw_hash_source_and_reference_bindings_are_mandatory(self):
        one, oracle, _ = self.evaluated_case()
        replay.validate_oracle_record(one, oracle)
        for mutation in ("hash", "source", "text", "missing", "utf8-flag"):
            changed = copy.deepcopy(oracle)
            if mutation == "hash":
                changed["utf8"]["stdout_sha256"] = "0" * 64
            elif mutation == "source":
                changed["source_sha256"] = "0" * 64
            elif mutation == "text":
                changed["utf8"]["stdout_bytes_hex"] = b"unrelated".hex()
                changed["utf8"]["stdout_sha256"] = replay.sha(b"unrelated")
            elif mutation == "missing":
                del changed["tree"]
            else:
                changed["utf8"].update(stdout="�", stdout_bytes_hex="ff",
                                       stdout_sha256=replay.sha(b"\xff"), utf8_valid=True)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                replay.validate_oracle_record(one, changed)
        invalid = copy.deepcopy(oracle)
        invalid["utf8"].update(stdout="�", stdout_bytes_hex="ff",
                               stdout_sha256=replay.sha(b"\xff"), utf8_valid=False)
        replay.validate_oracle_record(one, invalid)
        self.assertEqual(replay.admission(invalid), "generator-invalid")
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "oracle-cache-pinned"
            directory.mkdir()
            (directory / "cache.jsonl").write_text(json.dumps(oracle) + "\n")
            binding = {"identity": "pinned", "reference_sha256": "a" * 64}
            manifest = dict(binding, expectations_from_product=False)
            (directory / "manifest.json").write_text(json.dumps(manifest))
            replay.validated_cache(Path(temporary), binding, [one])
            manifest["reference_sha256"] = "b" * 64
            (directory / "manifest.json").write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, "active pristine"):
                replay.validated_cache(Path(temporary), binding, [one])

    def test_partial_oracle_cannot_supply_gold_but_process_failures_are_counted(self):
        one, oracle, product = self.evaluated_case()
        oracle["utf8"]["code"] = -11
        product["region"] = asserted(["wrong"])
        product["product_error"] = True
        with patch.object(replay.regions, "select_native_region", return_value=asserted(["A", "AFTER"])):
            record = replay.evaluate(one, oracle, product, {}, {})
        self.assertEqual((record["verdict"], record["failures"]), ("review", []))
        self.assertTrue(record["product_execution_error"])
        self.assertFalse(record["raw_axes"]["content"])
        summary = replay.summarize([record])
        self.assertEqual(summary["product_execution_error_sources"], 1)

    def test_after_cannot_impersonate_an_accepted_prefix_and_missing_axes_stay_open(self):
        one, oracle, product = self.evaluated_case()
        product["region"] = asserted(["AFTER"])
        one["axes"] = ["independent-consumer-proof"]
        with patch.object(replay.regions, "select_native_region", return_value=asserted(["A", "AFTER"])):
            record = replay.evaluate(one, oracle, product, {}, {})
        self.assertFalse(record["axes"]["accepted_prefix"])
        self.assertIn("accepted_prefix", record["failures"])
        self.assertEqual(record["axes"]["independent-consumer-proof"], "uncovered")

    def test_authored_scalars_controls_and_unicode_are_not_erased(self):
        correct = asserted(["A中e\u0301 B"])
        for wrong in ["A中e B", "A文e\u0301 B", "A中e\u0301\x00 B", "A中e\u0301 �B"]:
            with self.subTest(wrong=wrong):
                self.assertFalse(comparison.compare_axes(correct, asserted([wrong]))["content"])

    def test_word_join_and_nbsp_mutations_remain_observable(self):
        correct = asserted(["X BODY"])
        self.assertFalse(comparison.compare_axes(correct, asserted(["XBODY"]))["separators"])
        self.assertFalse(comparison.compare_axes(asserted(["X\u00a0BODY"]), correct)["content"])

    def test_leading_interior_trailing_and_repeated_hard_rows_are_independent(self):
        correct = ["", "X", "", "Y", "", ""]
        for index in range(len(correct)):
            if correct[index]:
                continue
            with self.subTest(row=index):
                wrong = correct[:index] + correct[index + 1:]
                self.assertFalse(comparison.compare_axes(asserted(correct), asserted(wrong))["rows"])
        self.assertFalse(comparison.compare_axes(asserted(correct), asserted(correct + [""]))["rows"])

    def test_repeated_link_identity_and_order_are_not_deduplicated(self):
        def link(uri):
            return {"type": "link", "target": {"kind": "external", "uri": uri}, "children": []}
        a, b = "https://example.org/a", "https://example.org/b"
        children = [link(a), link(a), link(b)]
        expected = [a, a, b]
        self.assertEqual(comparison.product_external_targets({"document": {"sections": children}}), expected)
        for wrong in [children[1:], children[::-1]]:
            self.assertNotEqual(comparison.product_external_targets({"document": {"sections": wrong}}), expected)

    def test_diagnostics_and_missing_extraction_never_turn_into_a_clean_pass(self):
        valid = {profile: {"code": 0} for profile in ("ascii", "utf8", "html", "tree", "lint")}
        self.assertEqual(replay.admission(valid), "valid")
        valid["lint"]["code"] = 1
        self.assertEqual(replay.admission(valid), "diagnosed-valid")
        for code in (-11, None, 3):
            invalid = {key: dict(value) for key, value in valid.items()}
            invalid["utf8"]["code"] = code
            self.assertEqual(replay.admission(invalid), "generator-invalid")
        report = comparison.compare_axes({"status": "uncovered"}, asserted(["BODY"]))
        verdict, failures = comparison.verdict("valid", report, {}, ["rows"])
        self.assertEqual(verdict, "review")
        self.assertEqual(failures, [])
        verdict, failures = comparison.verdict("diagnosed-valid", {"content": False}, {}, [])
        self.assertEqual((verdict, failures), ("fail", ["content"]))
        for mutation in ({"code": -11}, {"code": None}, {"code": 0, "timeout": True},
                         {"code": 0, "utf8_valid": False}):
            invalid = {key: dict(value) for key, value in valid.items()}
            invalid["lint"] = mutation
            self.assertEqual(replay.admission(invalid), "generator-invalid")

    def test_invalid_json_shapes_are_case_failures_and_do_not_abort_the_ledger(self):
        case = {"source": "input", "aliases": [{"cohort": "field", "family": "fixture"}]}
        for encoded in ('[]', '0', '{"document": []}', '{"document":{"diagnostics":null}}'):
            profile = {"code": 0, "timeout": False, "utf8_valid": True,
                       "stdout": encoded, "stderr": ""}
            with self.subTest(encoded=encoded), patch.object(transport, "run", return_value=profile):
                result, _ = replay.execute(Path("binary"), case)
                self.assertTrue(result["product_error"])
                self.assertIsNotNone(result["json_error"])

    def test_markdown_readback_uses_the_supported_plain_text_consumer(self):
        case = {"source": "input", "aliases": [{"cohort": "markdown", "family": "fixture"}]}
        profile = {"code": 0, "timeout": False, "utf8_valid": True,
                   "stdout": '{"document": {"sections": [], "diagnostics": []}}', "stderr": ""}
        with patch.object(transport, "run", return_value=profile) as run:
            replay.execute(Path("binary"), case)
        reader = next(call.args[1] for call in run.call_args_list
                      if "markdown" in call.args[1] and "text" in call.args[1])
        self.assertIn("direct", reader)
        self.assertNotIn("man", reader)

    def test_timeout_and_invalid_utf8_retain_actual_status_and_bytes(self):
        with patch.object(transport.subprocess, "run", side_effect=subprocess.TimeoutExpired(
                ["binary"], 1, output=b"partial\xff", stderr=b"detail")):
            result = transport.run(Path("binary"), [], "input", timeout=1)
        self.assertTrue(result["timeout"])
        self.assertIsNone(result["code"])
        self.assertFalse(result["utf8_valid"])
        self.assertEqual(bytes.fromhex(result["stdout_bytes_hex"]), b"partial\xff")

    def test_coverage_false_positive_and_omission_are_both_failed_axes(self):
        # Coverage is independent of ordinary native warning severity.
        for expected, actual in itertools.product((False, True), repeat=2):
            report = {"content-coverage": actual == expected}
            verdict, _ = comparison.verdict("valid", report, {}, [])
            self.assertEqual(verdict == "fail", expected != actual)


if __name__ == "__main__":
    unittest.main()
