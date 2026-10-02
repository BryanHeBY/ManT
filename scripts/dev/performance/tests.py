"""Meaningful statistics, identity, scheduling and failure-preservation tests."""

import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from .cards import build_card_admission, command, file_identity, fixed_input, measurement_scope, operation_harness_identity, sha256
from .runner import Plan, run, run_case, summarize
from .statistics import paired_summary, quantile
from .transport import collect, operation_metrics, wait_with_deadline


def build_card(artifact):
    return {"revision": "recorded", "compiler": "recorded", "nativeCompiler": "recorded",
            "profile": "release", "features": ["default"], "cargoLockSha256": "0" * 64,
            "artifacts": {"cli": artifact}}


def operation_payload(operation="load"):
    stages = ["parseOwned", "sourceLessLoweringAndRecognition", "nativeTextRender",
              "sourceLessDrop", "sourceAwareBytesLoad"]
    scope = ["source", "initialLoad", *stages, "combination"] if operation == "phase" else [
        "source", "initialLoad", "resultAllocation", "resultDestruction", "serialization", "index", "warmup"]
    result = {"schema": "mant.operation-measurement/v1", "operation": operation, "rounds": 7,
              "scope": {key: "recorded scope" for key in scope}}
    if operation == "phase":
        result["phaseMilliseconds"] = {key: [index + 1] * 7 for index, key in enumerate(stages)}
    else:
        result["milliseconds"] = [1000, 1000, 1, 2, 3, 4, 5]
    return result


class StatisticalContract(unittest.TestCase):
    def test_signed_differences_ignore_ab_ba_execution_order(self):
        result = paired_summary([[(10, 14), (100, 104)], [(25, 29), (2, 6)]], [(10, 10), (30, 30)], resamples=500)
        self.assertEqual(result["pairedDifferences"], [4] * 4)
        self.assertEqual(result["bootstrap95"], [4, 4])
        self.assertEqual(result["status"], "stable")
        self.assertEqual(result["candidate"]["maximum"], 104)

    def test_noise_and_batch_disagreement_prevent_optimization_claim(self):
        for batches, control in [([[(10, 11)], [(20, 21)]], [(10, 12)]),
                                 ([[(10, 11)], [(20, 19)]], [(10, 10)])]:
            with self.subTest(batches=batches):
                result = paired_summary(batches, control, resamples=500)
                self.assertEqual(result["status"], "noise-or-uncertain")

    def test_batch_weights_and_seed_are_preserved(self):
        batches = [[(10, 11)], [(10, 17)] * 9]
        first = paired_summary(batches, [(4, 4)], seed=9, resamples=500)
        second = paired_summary(batches, [(4, 4)], seed=9, resamples=500)
        self.assertEqual(first, second)
        self.assertEqual(first["medianDifference"], 7)
        self.assertEqual(first["bootstrap95"], [7, 7])

    def test_zero_baseline_and_invalid_values_are_explicit(self):
        result = paired_summary([[(0, 2)], [(0, 3)]], [(0, 0)], resamples=100)
        self.assertEqual(result["pairedRelativeDifferences"], [None, None])
        self.assertEqual(result["undefinedRelativeDifferences"], 2)
        for value in [float("nan"), float("inf"), -1, True]:
            self.assertEqual(paired_summary([[(value, 3)]], [(1, 1)], resamples=10)["status"], "invalid")

    def test_percentile_interpolation_is_frozen(self):
        self.assertAlmostEqual(quantile([0, 10], .95), 9.5)


class SamplingContract(unittest.TestCase):
    def test_formal_plan_has_twenty_pairs_ten_controls_and_six_warmups(self):
        plan = Plan()
        rows = list(plan.invocations())
        self.assertTrue(plan.formal_defaults)
        self.assertEqual(len(rows), 66)
        self.assertEqual(sum(row["kind"] == "warmup" for row in rows), 6)
        self.assertEqual(sum(row["kind"] == "paired" for row in rows), 40)
        self.assertEqual(sum(row["kind"] == "aa" for row in rows), 20)
        self.assertEqual([row["placement"] for row in rows if row["kind"] == "aa"], ["before"] * 10 + ["after"] * 10)
        for batch in [0, 1]:
            pairs = [row for row in rows if row.get("batch") == batch and row["ordinal"] == 0]
            self.assertEqual([row["order"] for row in pairs], ["AB", "BA"] * 5)
        self.assertFalse(Plan(1, 0, 2).formal_defaults)

    def test_failed_controls_warmups_and_missing_pairs_are_not_dropped(self):
        plan = Plan(1, 1, 2, resamples=50)
        rows = [{**item, "status": "ok", "metrics": {"process.wallMs": 10}} for item in plan.invocations()]
        self.assertEqual(summarize(rows, plan)["status"], "measured")
        for index in [0, 2, 6]:
            broken = [dict(row) for row in rows]
            broken[index]["status"] = "timeout"
            result = summarize(broken, plan)
            self.assertEqual(result["status"], "incomplete")
            self.assertEqual(result["metrics"]["process.wallMs"]["status"], "incomplete")
        self.assertEqual(summarize(rows[:-1], plan)["status"], "incomplete")
        duplicate = [dict(row) for row in rows]
        duplicate[3] = duplicate[2]
        self.assertFalse(summarize(duplicate, plan)["invocationPlanMatches"])

    def test_seven_operation_rounds_reduce_to_one_last_five_sample(self):
        payload = operation_payload()
        self.assertEqual(operation_metrics(payload), {"operation.operation.wallMs": 3})
        for bad in [{**payload, "milliseconds": [1] * 6}, {**payload, "milliseconds": [1] * 6 + [float("nan")]}, {"operation": "load", "milliseconds": [1] * 7}]:
            with self.assertRaises(ValueError):
                operation_metrics(bad)
        phase = operation_payload("phase")
        self.assertEqual(operation_metrics(phase), {f"operation.{key}.wallMs": index + 1 for index, key in enumerate(phase["phaseMilliseconds"])})

    def test_operation_scope_and_complete_phase_vectors_are_required(self):
        phase = operation_payload("phase")
        bad_vectors = [dict(phase["phaseMilliseconds"]), dict(phase["phaseMilliseconds"])]
        del bad_vectors[0]["sourceLessDrop"]
        bad_vectors[1]["inventedStage"] = [1] * 7
        malformed = [{**phase, "phaseMilliseconds": value} for value in bad_vectors]
        malformed.extend({**phase, "rounds": rounds} for rounds in [None, True, 6, 7.0])
        malformed.extend({**phase, "scope": scope} for scope in [None, {}, {"source": "recorded"},
                         {key: "" for key in phase["scope"]}, {key: None for key in phase["scope"]}])
        for payload in malformed:
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                operation_metrics(payload)


class SourceAndTransportContract(unittest.TestCase):
    def test_build_identity_never_falls_back_to_current_head(self):
        artifact = {"sha256": "fixed"}
        self.assertEqual(build_card_admission({}, {"cli": artifact})["status"], "incomplete")
        card = build_card(artifact)
        self.assertEqual(build_card_admission(card, {"cli": artifact})["status"], "bound")
        self.assertEqual(build_card_admission(card, {"cli": {"sha256": "different"}})["unboundArtifacts"], ["cli"])
        card["artifacts"]["operation"] = artifact
        self.assertIn("operation-harness-source", build_card_admission(card, {"operation": artifact})["unboundArtifacts"])
        card["operationHarnessSha256"] = operation_harness_identity()["sha256"]
        self.assertEqual(build_card_admission(card, {"operation": artifact})["status"], "bound")

    def test_invalid_build_metadata_cannot_be_bound_by_artifact_hash(self):
        artifact = {"sha256": "fixed"}
        for field, values in {
            "revision": [None, "", "  ", 3], "compiler": [None, ""],
            "nativeCompiler": [None, ""], "profile": [None, ""],
            "features": [None, "default", [None], [""]],
            "cargoLockSha256": [None, "", "not a hash", "0" * 63],
        }.items():
            for value in values:
                card = {**build_card(artifact), field: value}
                with self.subTest(field=field, value=value):
                    admission = build_card_admission(card, {"cli": artifact})
                    self.assertEqual(admission["status"], "incomplete")
                    self.assertIn(field, admission["invalidFields"])
        self.assertEqual(build_card_admission(None, {"cli": artifact})["status"], "incomplete")
        self.assertEqual(build_card_admission({**build_card(artifact), "features": []}, {"cli": artifact})["status"], "bound")

    def test_nonfinite_deadlines_stop_before_creating_evidence(self):
        from argparse import Namespace
        with tempfile.TemporaryDirectory() as directory:
            for value in [float("nan"), float("inf"), 1e300, 0, -1]:
                output = Path(directory) / "unused"
                with self.subTest(value=value), self.assertRaises(ValueError):
                    run(Namespace(timeout=value, output=output))
                self.assertFalse(output.exists())
                with patch("scripts.dev.performance.transport.threading.Timer") as timer:
                    with self.assertRaises(ValueError):
                        wait_with_deadline(None, value)
                    timer.assert_not_called()

    def test_input_mutation_or_removal_invalidates_finished_samples(self):
        from argparse import Namespace
        for kind, remove in [("stored", False), ("operationInput", False), ("stored", True)]:
            with self.subTest(kind=kind, remove=remove), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / "binary"
                binary.write_bytes(b"not run")
                input_card = {"id": "gcc"}
                for name in ("stored", "operationInput"):
                    path = root / name
                    path.write_bytes(b"locked input")
                    input_card[name] = file_identity(path)
                build = root / "build.json"
                build.write_text(json.dumps(build_card(file_identity(binary))))
                args = Namespace(output=root / "new", baseline=binary, candidate=binary,
                                 baseline_build_card=build, candidate_build_card=build,
                                 baseline_operations=None, candidate_operations=None,
                                 pairs=1, warmups=0, aa_pairs=2, timeout=3,
                                 panel="cli", pages=["gcc"], modes=["text"])
                def mutate(*_args):
                    path = Path(input_card[kind]["path"])
                    path.unlink() if remove else path.write_bytes(b"mutated input")
                    return {"status": "measured", "retained": True}
                with patch("scripts.dev.performance.runner.fixed_input", return_value=input_card), patch("scripts.dev.performance.runner.run_case", side_effect=mutate):
                    with self.assertRaises(SystemExit) as failure:
                        run(args)
                    self.assertEqual(failure.exception.code, 1)
                summary = json.loads((args.output / "summary.json").read_text())
                self.assertEqual(summary["status"], "incomplete")
                self.assertTrue(summary["inputDrift"])
                self.assertFalse(summary["artifactDrift"])
                self.assertTrue(summary["cases"][0]["retained"])
                self.assertEqual(json.loads((args.output / "manifest.json").read_text())["timeoutSeconds"], 3)

    def test_unbound_build_card_stops_before_measurement(self):
        from argparse import Namespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "binary"
            binary.write_bytes(b"not run")
            build = root / "build.json"
            build.write_text("{}")
            args = Namespace(output=root / "new", baseline=binary, candidate=binary,
                             baseline_build_card=build, candidate_build_card=build,
                             baseline_operations=None, candidate_operations=None,
                             pairs=1, warmups=0, aa_pairs=2, timeout=1,
                             panel="cli", pages=["gcc"], modes=["text"])
            with patch("scripts.dev.performance.runner.fixed_input", return_value={"id": "gcc"}), patch("scripts.dev.performance.runner.run_case") as process:
                with self.assertRaises(SystemExit) as failure:
                    run(args)
                self.assertEqual(failure.exception.code, 1)
                process.assert_not_called()
            self.assertEqual(json.loads((args.output / "summary.json").read_text())["status"], "incomplete")

    def test_existing_evidence_is_not_overwritten(self):
        import subprocess
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            sentinel = destination / "collection-error.json"
            sentinel.write_text("retained earlier evidence")
            result = subprocess.run([sys.executable, "-m", "scripts.dev.performance.runner",
                                     "--baseline", "missing", "--candidate", "missing",
                                     "--baseline-build-card", "missing", "--candidate-build-card", "missing",
                                     "--output", str(destination)], capture_output=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(sentinel.read_text(), "retained earlier evidence")

    def test_fixture_check_preserves_crlf_and_rejects_mutation(self):
        raw = b".TH TEST 1\r\n.SH BODY\r\n"
        stored = b"locked container"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "tests/fixtures/roff/real/sample/page.zst"
            path.parent.mkdir(parents=True)
            path.write_bytes(stored)
            (path.parent / "README.md").write_text("Fixed source provenance.\n")
            output = root / "inputs"
            output.mkdir()
            with patch("scripts.dev.performance.cards.FIXTURES", {"test": ("sample/page.zst", sha256(stored), sha256(raw))}), patch("scripts.dev.performance.cards.subprocess.run") as process:
                process.return_value.returncode = 0
                process.return_value.stdout = raw
                card = fixed_input("test", output, root=root)
                import gzip
                self.assertEqual(gzip.decompress(Path(card["operationInput"]["path"]).read_bytes()), raw)
                path.write_bytes(b"changed")
                with self.assertRaises(ValueError):
                    fixed_input("test", output, root=root)

    def test_modes_and_resource_scope_do_not_claim_native_parse_cpu(self):
        source = {"stored": {"path": "original.zst"}, "operationInput": {"path": "decoded.gz"}}
        self.assertIn("original.zst", command("mant", source, "cli", "text"))
        self.assertEqual(command("measure", source, "operation", "search"), ["measure", "decoded.gz", "search", "option"])
        self.assertIn("not operation-exclusive", measurement_scope("operation", "load")["processResources"])

    def test_process_failures_keep_bytes_and_status(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            record = collect([sys.executable, "-c", "import sys; print('retained'); print('reason',file=sys.stderr);sys.exit(7)"], destination, "failure", panel="cli", timeout=5, capture=True)
            self.assertEqual(record["exitCode"], 7)
            self.assertEqual(record["status"], "exit-error")
            self.assertEqual(record["timeoutSeconds"], 5)
            self.assertEqual(Path(record["stdout"]["path"]).read_text(), "retained\n")
            self.assertEqual(Path(record["stderr"]["path"]).read_text(), "reason\n")
            missing = collect([str(destination / "missing")], destination, "missing", panel="cli", timeout=5)
            self.assertEqual(missing["status"], "spawn-error")

    def test_timeout_and_bad_operation_are_raw_failures(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            timeout = collect([sys.executable, "-c", "import time;print('started',flush=True);time.sleep(5)"], destination, "timeout", panel="cli", timeout=.1, capture=True)
            self.assertEqual(timeout["status"], "timeout")
            self.assertEqual(Path(timeout["stdout"]["path"]).read_text(), "started\n")
            bad = collect([sys.executable, "-c", "print('{}')", "load", "-h"], destination, "bad", panel="operation", timeout=5)
            self.assertEqual(bad["status"], "operation-error")
            self.assertIn("operationError", bad)

    def test_wall_wait_uses_blocking_wait_and_independent_watchdog(self):
        with patch("scripts.dev.performance.transport.threading.Timer") as timer:
            from unittest.mock import Mock
            process = Mock()
            process.returncode = 0
            process.wait.return_value = 0
            code, expired = wait_with_deadline(process, .1)
            self.assertEqual(code, 0)
            self.assertFalse(expired)
            process.wait.assert_called_once_with()
            timer.return_value.start.assert_called_once_with()
            timer.return_value.cancel.assert_called_once_with()
            timer.return_value.join.assert_called_once_with()

    def test_timed_cli_output_is_discarded_and_correctness_is_independent(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "fake"
            fake.write_text(f"#!{sys.executable}\nprint('small durable output')\n")
            fake.chmod(0o755)
            card = {"panel": "cli", "mode": "text", "input": {"stored": {"path": "source"}}}
            result = run_case(card, {"baseline": fake, "candidate": fake}, root / "case", Plan(1, 0, 2, resamples=10), 5, None)
            self.assertEqual(result["status"], "measured")
            evidence = json.loads((root / "case/correctness.json").read_text())
            self.assertIn("unreviewed", evidence["classification"])
            self.assertTrue((root / "case/correctness-baseline.stdout").exists())
            samples = [json.loads(row) for row in (root / "case/samples.jsonl").read_text().splitlines()]
            self.assertTrue(all(row["stdoutDestination"] == "/dev/null" and "stdout" not in row for row in samples))
            self.assertFalse((root / "case/0000.stdout").exists())


if __name__ == "__main__":
    unittest.main()
