"""Invocation provenance and source/recipe cache identity regressions."""

from collections import Counter
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts.roff.fixtures import generate_roff_compatibility_fixtures as recorder
from scripts.roff.fixtures import reference_recipes as recipes
from scripts.roff.fixtures import replay_roff_acceptance as transport
from scripts.roff.fixtures import replay_rule_boundaries as replay
from scripts.roff.fixtures import record_field_rule_matrix as field_recorder
from scripts.roff.fixtures import record_integrity_rule_matrix as integrity_recorder
from scripts.roff.fixtures import record_rule_projection_cards as card_recorder
from scripts.roff.fixtures import acceptance_comparison as comparison


class ReferenceRecipeTests(unittest.TestCase):
    @staticmethod
    def case(cohort="fixture"):
        source = '.TH PROBE 1 "October 2, 2026"\n.SH DESCRIPTION\nBODY\n'
        return dict(id="recipe-test", cohort=cohort, source=source,
                    source_sha256=hashlib.sha256(source.encode()).hexdigest())

    @staticmethod
    def recorded(case, reference=Path("/reference/mandoc")):
        completed = subprocess.CompletedProcess([], 0, b"BODY\n", b"")
        with patch.object(transport.subprocess, "run", return_value=completed):
            return transport.record_oracle(reference, case)

    @staticmethod
    def store(directory, records, manifest=None):
        path = directory / "oracle-cache-pinned"
        path.mkdir(exist_ok=True)
        (path / "cache.jsonl").write_text("".join(json.dumps(r) + "\n" for r in records))
        binding = dict(identity="pinned", reference_sha256="a" * 64,
                       reference_path="/reference/mandoc",
                       expectations_from_product=False, **recipes.cache_binding())
        (path / "manifest.json").write_text(json.dumps(binding if manifest is None else manifest))
        return path

    def test_recorder_and_replay_keep_their_actual_profile_arguments(self):
        # Before these assertions: exact pinned main.c -I/post_Os/post_TH
        # sources ran all five profiles. An authored OS precedes -Ios; an
        # empty OS uses it. Do not normalize either footer after rendering.
        for profile in recipes.PROFILES:
            legacy = ["-T" + profile, "-Owidth=78", "-Ios=Historical Oracle Footer"]
            default = ["-T" + profile] + (["-Owidth=78"] if profile in ("ascii", "utf8") else [])
            self.assertEqual(recipes.arguments(profile, recipes.RECORDER), legacy)
            self.assertEqual(recipes.arguments(profile), default)
            completed = subprocess.CompletedProcess([], 0, b"BODY\n", b"")
            with patch.object(recorder.roff_fixture_reference, "run_reference", return_value=completed) as run:
                recorder.record_profiles("fixture", "recipe", self.case()["source"], selected=[profile])
                self.assertEqual(run.call_args.args[1], legacy)
                self.assertEqual(run.call_args.kwargs["env"], recipes.environment())

    def test_existing_receipts_select_recipes_before_candidate_execution(self):
        cases = replay.assemble(list(replay.COHORTS))
        self.assertEqual(Counter(recipes.for_case(one) for one in cases),
                         {recipes.DEFAULT: 7577, recipes.RECORDER: 2458})
        self.assertEqual(len({one["source_sha256"] for one in cases}), 8862)
        self.assertEqual(len({recipes.key(one) for one in cases}), 9035)
        receipts = recipes.recorder_receipts()
        self.assertEqual(len(receipts), 30)
        one = next(one for one in cases if one["id"] in receipts)
        with self.assertRaisesRegex(ValueError, "receipt"):
            recipes.for_case(dict(one, source=one["source"] + "changed"))

    def test_same_source_different_recipes_never_overwrite_each_other(self):
        plain, legacy = self.case(), self.case("escape")
        records = [self.recorded(plain), self.recorded(legacy)]
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            self.store(directory, records)
            _, _, cache = transport.load_cache(directory, "pinned", "a" * 64)
            self.assertEqual(len(cache), 2)
            self.assertEqual(transport.oracle_record(cache, plain), records[0])
            self.assertEqual(transport.oracle_record(cache, legacy), records[1])
            changed = copy.deepcopy(records[0])
            changed["utf8"].update(stdout="OTHER\n", stdout_sha256=transport.digest(b"OTHER\n"))
            self.store(directory, [*records, changed])
            with self.assertRaisesRegex(ValueError, "conflicting"):
                transport.load_cache(directory, "pinned", "a" * 64)

    def test_old_or_mutated_manifest_cannot_authenticate_a_cache(self):
        old = dict(identity="pinned", reference_sha256="a" * 64, expectations_from_product=False)
        current = dict(old, reference_path="/reference/mandoc", **recipes.cache_binding())
        mutations = [old, dict(current, recipeCatalog={}), dict(current, receiptBinding={}),
                     dict(current, reference_sha256="b" * 64)]
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for manifest in mutations:
                path = self.store(directory, [self.recorded(self.case())], manifest)
                before = (path / "cache.jsonl").read_bytes()
                with self.subTest(manifest=manifest), self.assertRaises(ValueError):
                    transport.load_cache(directory, "pinned", "a" * 64)
                self.assertEqual((path / "cache.jsonl").read_bytes(), before)

    def test_profile_argv_environment_input_and_bytes_are_bound(self):
        original = self.recorded(self.case("escape"))
        recipes.validate_record(original, "/reference/mandoc")
        for field, value in [("arguments", ["-Tutf8"]), ("argv", ["mandoc", "-Tutf8"]),
                             ("environment", {}), ("stdinSha256", "b" * 64),
                             ("timeoutSeconds", 99), ("stdout", "FORGED")]:
            changed = copy.deepcopy(original)
            changed["utf8"][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                recipes.validate_record(changed, "/reference/mandoc")

    def test_original_executable_path_is_bound_even_for_a_copied_cache(self):
        record = self.recorded(self.case())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = self.store(root, [record])
            # Evidence can move and its original executable need not remain
            # installed. The active caller still authenticates binary SHA;
            # argv must retain this cache producer's actual invocation path.
            transport.load_cache(root, "pinned", "a" * 64)
            for profiles in [("utf8",), recipes.PROFILES]:
                changed = copy.deepcopy(record)
                for profile in profiles:
                    changed[profile]["argv"][0] = "/other/not-mandoc"
                (directory / "cache.jsonl").write_text(json.dumps(changed) + "\n")
                with self.subTest(profiles=profiles), self.assertRaisesRegex(ValueError, "invocation"):
                    transport.load_cache(root, "pinned", "a" * 64)
            self.store(root, [record])
            manifest = json.loads((directory / "manifest.json").read_bytes())
            for reference_path in (None, "mandoc", 0):
                (directory / "manifest.json").write_text(json.dumps(dict(manifest, reference_path=reference_path)))
                with self.subTest(reference_path=reference_path), self.assertRaisesRegex(ValueError, "invocation path"):
                    transport.load_cache(root, "pinned", "a" * 64)

    def test_collectors_share_recipe_records_and_preserve_partial_raw_bytes(self):
        one = self.case("escape")
        completed = subprocess.CompletedProcess([], 3, b"A\xff", b"diagnostic")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / "mandoc"
            reference.write_bytes(b"bound-test-binary")
            for collector, directory in [(replay.collect_reference, root / "current"),
                                         (transport.collect_oracle, root / "historical")]:
                with patch.object(transport.subprocess, "run", return_value=completed):
                    collector(reference, {"identity": "pinned"}, [one], directory, 1)
                _, _, cache = transport.load_cache(directory, "pinned", transport.digest(reference.read_bytes()))
                record = transport.oracle_record(cache, one)
                self.assertFalse(record["utf8"]["utf8_valid"])
                self.assertEqual(record["utf8"]["stdout_bytes_hex"], "41ff")
                self.assertEqual(record["utf8"]["stdout_sha256"], transport.digest(b"A\xff"))
                self.assertEqual(record["utf8"]["code"], 3)

    def test_collectors_reject_a_changed_invocation_path_before_any_cache_mutation(self):
        one = self.case()
        other = dict(one, id="other", source=one["source"] + "MORE\n")
        other["source_sha256"] = transport.digest(other["source"].encode())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original, active = root / "original-mandoc", root / "active-mandoc"
            original.write_bytes(b"same-bound-binary")
            active.write_bytes(original.read_bytes())
            binary_sha = transport.digest(active.read_bytes())
            record = self.recorded(one, original)
            manifest = dict(identity="pinned", reference_sha256=binary_sha,
                            reference_path=str(original.resolve()),
                            expectations_from_product=False, **recipes.cache_binding())
            for collector in (replay.collect_reference, transport.collect_oracle):
                for cases in ([one], [one, other]):
                    with self.subTest(collector=collector.__name__, missing=len(cases) - 1):
                        evidence = root / "evidence"
                        evidence.mkdir(exist_ok=True)
                        cache = self.store(evidence, [record], manifest)
                        before = {path.name: path.read_bytes() for path in cache.iterdir()}
                        # Reading moved evidence is valid; appending with a new
                        # executable path must never rewrite its producer facts.
                        transport.load_cache(evidence, "pinned", binary_sha)
                        with patch.object(transport.subprocess, "run") as run:
                            with self.assertRaisesRegex(ValueError, "fresh directory"):
                                collector(active, {"identity": "pinned"}, cases, evidence, 1)
                            run.assert_not_called()
                        self.assertEqual({path.name: path.read_bytes() for path in cache.iterdir()}, before)

    def test_process_status_types_and_timeout_pairs_are_authenticated(self):
        original = self.recorded(self.case())
        for field, value in [("code", True), ("code", None), ("code", "0"),
                             ("timeout", 0), ("timeout", True), ("utf8_valid", 1)]:
            changed = copy.deepcopy(original)
            changed["utf8"][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                recipes.validate_record(changed, "/reference/mandoc")
            self.assertEqual(replay.admission(changed), "generator-invalid")
        for flag in ("timeout", "utf8_valid"):
            changed = copy.deepcopy(original)
            del changed["utf8"][flag]
            # Old immutable receipts omit flags; persisted v2 records cannot.
            recipes.validate_streams(changed["source_sha256"], changed)
            with self.assertRaisesRegex(ValueError, "process flags"):
                recipes.validate_record(changed, "/reference/mandoc")

    def test_timed_out_partial_profiles_remain_evidence_not_valid_sources(self):
        one = dict(self.case(), family="probe", layout="inline")
        timeout = subprocess.TimeoutExpired("mandoc", 30, output=b"A\xff", stderr=b"partial")
        with patch.object(transport.subprocess, "run", side_effect=timeout):
            record = transport.record_oracle(Path("/reference/mandoc"), one)
        recipes.validate_record(record, "/reference/mandoc")
        self.assertIsNone(record["utf8"]["code"])
        self.assertIs(record["utf8"]["timeout"], True)
        self.assertEqual(record["utf8"]["stdout_bytes_hex"], "41ff")
        self.assertEqual(replay.admission(record), "generator-invalid")
        self.assertEqual(transport.admit(one, {recipes.key(one): record})["admission"], "generator-defect")

    def test_partial_ascii_and_html_profiles_cannot_qualify_historical_comparisons(self):
        one = dict(self.case(), family="probe", layout="inline")
        original = self.recorded(one)
        for profile in recipes.PROFILES:
            changed = copy.deepcopy(original)
            changed[profile].update(code=None, timeout=True)
            recipes.validate_record(changed, "/reference/mandoc")
            with self.subTest(profile=profile):
                self.assertEqual(replay.admission(changed), "generator-invalid")
                self.assertEqual(transport.admit(one, {recipes.key(one): changed})["admission"],
                                 "generator-defect")

    def test_historical_ledger_withholds_partial_gold_but_retains_failed_observations(self):
        # Classifier mutation, not native formatting gold: differing rows and
        # targets deliberately exercise the public ledger's qualification.
        one = dict(self.case(), family="probe", layout="inline", policy={"identity": "plain"})
        original = self.recorded(one)
        native = {"status": "asserted", "rows": ["EXPECTED"]}
        product = {"product_region": {"status": "asserted", "rows": ["DIFFERENT"]},
                   "external_targets": ["https://example.org"], "product_error": True}

        def row(record):
            with patch.object(transport.acceptance_regions, "select_native_region", return_value=native):
                ledger = transport.build_ledger([one], {recipes.key(one): record},
                                                {one["source_sha256"]: product}, {}, None)
            return ledger["cases"][one["id"]]

        for profile in recipes.PROFILES:
            partial = copy.deepcopy(original)
            partial[profile].update(code=None, timeout=True)
            recipes.validate_record(partial, "/reference/mandoc")
            with self.subTest(profile=profile):
                observed = row(partial)
                self.assertEqual(observed["status"], "review")
                self.assertEqual(observed["failing_axes"], [])
                self.assertTrue(all(value == "uncovered" for value in observed["axis_report"].values()))
                self.assertIs(observed["unqualified_axis_observations"]["content"], False)
                self.assertIs(observed["unqualified_axis_observations"]["identity:external-occurrences"], False)
                self.assertIs(observed["product"]["product_error"], True)
        for lint_code in (0, 2, 3):
            admitted = copy.deepcopy(original)
            admitted["lint"]["code"] = lint_code
            observed = row(admitted)
            self.assertEqual(observed["status"], "fail")
            self.assertIn("content", observed["failing_axes"])
            self.assertNotIn("unqualified_axis_observations", observed)
        for status in comparison.UNQUALIFIED_ADMISSIONS:
            self.assertEqual(comparison.verdict(status, {"content": False}, {"product_error": True}, []),
                             ("review", []))

    def test_all_recorders_read_authenticated_recipe_keys_without_changing_gold_shape(self):
        # Transport-only regression: mock projection analysis, never derive a
        # roff expectation from these fake process bytes. Exercise the real
        # cache loader and public recording entrypoints, including the field
        # generator's single-pass iterator and unchanged frozen profile shape.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / "target/mandoc-migration/reference/mandoc"
            reference.parent.mkdir(parents=True)
            reference.write_bytes(b"bound-test-reference")
            binding = dict(identity="pinned", reference_sha256=transport.digest(reference.read_bytes()),
                           reference_path=str(reference.resolve()),
                           expectations_from_product=False, **recipes.cache_binding())
            plain = dict(self.case(), family="field-transition", metadata={})
            legacy = dict(plain, cohort="escape")
            records = [self.recorded(plain, reference), self.recorded(legacy, reference)]
            for profile in recipes.PROFILES:
                records[1][profile].update(stdout="RECORDER\n",
                                          stdout_sha256=transport.digest(b"RECORDER\n"))
            directory = self.store(root, records, binding)
            registration = {"identity": "pinned"}
            region = {"status": "asserted", "rows": ["BODY"]}
            card = {"rules": ["transport-only"], "native_utf8_sha256": records[0]["utf8"]["stdout_sha256"]}
            with patch.object(card_recorder, "ROOT", root), \
                    patch.object(card_recorder, "verified_reference", return_value=registration), \
                    patch.object(replay, "assemble", return_value=[plain]), \
                    patch.object(card_recorder.fields, "build_policy", return_value=(card, region)) as build:
                self.assertEqual(len(card_recorder.cards(root)), 1)
                self.assertEqual(build.call_args.args[1], records[0])
                special = dict(plain, id="coverage-control-source-spelling-fallback")
                with patch.object(replay, "assemble", return_value=[special]):
                    result = card_recorder.integrity_cards(root)
                self.assertEqual(result[0]["native_utf8_sha256"], records[0]["utf8"]["stdout_sha256"])
            with patch.object(integrity_recorder, "ROOT", root), \
                    patch.object(integrity_recorder, "verified_reference", return_value=registration), \
                    patch.object(replay, "assemble", return_value=[legacy]), \
                    patch.object(integrity_recorder.acceptance_regions, "select_native_region", return_value=region):
                result = integrity_recorder.record(root)
                self.assertEqual(result[0]["profile_hashes"]["utf8"]["stdout_sha256"],
                                 records[1]["utf8"]["stdout_sha256"])
            fixture = root / "field-gold.json.gz"
            with patch.object(field_recorder, "ROOT", root), \
                    patch.object(field_recorder, "FIXTURE", fixture), \
                    patch.object(field_recorder, "verified_reference", return_value=registration), \
                    patch.object(field_recorder, "cases", side_effect=lambda: iter([plain])), \
                    patch.object(field_recorder, "build_policy", return_value=(card, region)), \
                    patch.object(field_recorder, "project_regions", return_value=(region, [])), \
                    patch.object(field_recorder, "owner_witness", return_value={"reachable": True}), \
                    patch.object(sys, "argv", ["record-field", "--cache", str(directory)]):
                field_recorder.main()
            import gzip
            rows = [json.loads(line) for line in gzip.decompress(fixture.read_bytes()).splitlines()]
            self.assertEqual(rows[0]["count"], 1)
            self.assertEqual(len(rows), 2)
            self.assertEqual(set(rows[1]["profiles"]), set(recipes.PROFILES))
            self.assertEqual(list(rows[1]["profiles"]), ["ascii", "utf8", "html", "lint", "tree"])
            for profile in rows[1]["profiles"].values():
                self.assertEqual(set(profile), {"code", "stdout_sha256", "stderr_sha256"})


if __name__ == "__main__":
    unittest.main()
