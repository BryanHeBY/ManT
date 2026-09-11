"""Independent failure-path tests for the development rendering census."""
import argparse
import bz2
import gzip
import importlib.util
import json
import lzma
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from roff_review_queue import ArtifactSelection, classify, plain_output_controls

SPEC = importlib.util.spec_from_file_location("rendering_census", Path(__file__).with_name("audit-roff-rendering.py"))
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class RenderingCensusTests(unittest.TestCase):
    def inspect(self, source=b"BODY", reference=b"BODY", mant=b"BODY", raw_status="covered"):
        covered = {"status": "covered", "findings": [], "coverage": {"complete": True}}
        raw = {"status": raw_status, "findings": [], "coverage": {"complete": raw_status == "covered"}}
        args = argparse.Namespace(reference=Path("reference"), mant=Path("mant"), width=80, timeout=1)
        with patch.object(AUDIT, "source_bytes", return_value=(source, AUDIT.digest(source))), \
             patch.object(AUDIT, "run_renderer", side_effect=[(0, reference, ""), (0, mant, "")]), \
             patch.object(AUDIT, "prepare_frame", side_effect=lambda text, *_args, **_kwargs: (text, {"status": "covered"})), \
             patch.object(AUDIT, "compare_content", return_value=covered), \
             patch.object(AUDIT, "plain_output_controls", return_value=raw), \
             patch.object(AUDIT, "compare_layout_geometry", return_value=covered):
            return AUDIT.inspect(("input", [{"id": "one"}]), args)[0]

    def test_invalid_utf8_on_either_output_or_source_cannot_be_clean(self):
        self.assertEqual(self.inspect()["status"], "clean")
        for kwargs in ({"source": b"\xff"}, {"reference": b"\xff"}, {"mant": b"\xff"}):
            self.assertEqual(self.inspect(**kwargs)["status"], "review", kwargs)

    def test_uncovered_raw_control_check_cannot_be_clean(self):
        record = self.inspect(raw_status="uncovered")
        self.assertEqual(record["status"], "partial")
        self.assertEqual(record["rawControlCoverage"]["status"], "uncovered")

    def test_transport_hash_and_all_supported_inprocess_compressions(self):
        source = b".TH PROBE 1\nBODY\n"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source"
            for encoded in (source, gzip.compress(source), bz2.compress(source), lzma.compress(source),
                            lzma.compress(source[:5]) + b"\0" * 4 + lzma.compress(source[5:])):
                path.write_bytes(encoded)
                self.assertEqual(AUDIT.source_bytes(path), (source, AUDIT.digest(encoded)))
            path.write_bytes(lzma.compress(source)[:-1])
            with self.assertRaises(EOFError):
                AUDIT.source_bytes(path)

    def test_decoded_limit_and_xz_dictionary_limit_are_independent(self):
        with patch.object(AUDIT, "MAX_INPUT_BYTES", 4):
            with self.assertRaises(AUDIT.SourceBudgetError):
                AUDIT.decode_xz(lzma.compress(b"12345"))
        real = lzma.LZMADecompressor
        with patch.object(AUDIT.lzma, "LZMADecompressor", wraps=real) as decoder:
            self.assertEqual(AUDIT.decode_xz(lzma.compress(b"body")), b"body")
            self.assertEqual(decoder.call_args.kwargs["memlimit"], 64 * 1024 * 1024)

    def test_encoded_decoded_and_manifest_caps_are_typed_with_path_context(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'oversized-source'
            with patch.object(AUDIT, 'MAX_INPUT_BYTES', 64):
                for data in (b'x' * 65, gzip.compress(b'x' * 65), bz2.compress(b'x' * 65)):
                    with self.subTest(encoded_bytes=len(data)):
                        path.write_bytes(data)
                        with self.assertRaises(AUDIT.SourceBudgetError) as error:
                            AUDIT.source_bytes(path)
                        self.assertIn(str(path), str(error.exception))
                        self.assertIn('64 bytes', str(error.exception))
            path.write_bytes(b'12345')
            with patch.object(AUDIT, 'MAX_MANIFEST_BYTES', 4), self.assertRaises(AUDIT.SourceBudgetError) as error:
                AUDIT.census_inputs(argparse.Namespace(manifest=path, max_pages=None))
            self.assertIn(str(path), str(error.exception))
            self.assertIn('manifest', str(error.exception))

    def test_real_liblzma_memory_limit_is_typed_not_a_corrupt_source(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'small-output-large-dictionary.xz'
            path.write_bytes(lzma.compress(b'body'))
            # A tiny configured cap exercises the real liblzma error without
            # allocating a deliberately enormous compressor dictionary.
            with patch.object(AUDIT, 'XZ_MEMORY_LIMIT', 1024), self.assertRaises(AUDIT.SourceBudgetError) as error:
                AUDIT.source_bytes(path)
            self.assertIn(str(path), str(error.exception))
            self.assertIn('XZ dictionary memory cap (1024 bytes)', str(error.exception))
            self.assertIn('Memory usage limit exceeded', str(error.exception))
            self.assertIsInstance(error.exception.__cause__.__cause__, lzma.LZMAError)
            path.write_bytes(b'\xfd7zXZ\0corrupt')
            with self.assertRaises(lzma.LZMAError):
                AUDIT.source_bytes(path)

    def test_zstd_uses_frozen_path_and_retains_budget_exit_and_stderr(self):
        decoder = Path('/frozen/decoder/zstd')
        failures = [(124, 'renderer timed out after 10s'),
                    (125, 'renderer output exceeds 16777216 bytes'),
                    (-24, 'CPU limit'), (-9, 'resource kill')]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'source.zst'
            path.write_bytes(b'\x28\xb5\x2f\xfdencoded')
            for code, detail in failures:
                with self.subTest(code=code), patch.object(AUDIT, 'ZSTD_BINARY', decoder), \
                     patch.object(AUDIT, 'run_renderer', return_value=(code, b'', detail)) as run, \
                     self.assertRaises(AUDIT.SourceBudgetError) as error:
                    AUDIT.source_bytes(path)
                self.assertEqual(run.call_args.args[0], [str(decoder), '-qdc'])
                self.assertIn(str(path), str(error.exception))
                self.assertIn(f'exit {code}', str(error.exception))
                self.assertIn(detail, str(error.exception))
            with patch.object(AUDIT, 'ZSTD_BINARY', decoder), \
                 patch.object(AUDIT, 'run_renderer', return_value=(1, b'', 'corrupt frame')), \
                 self.assertRaises(ValueError) as error:
                AUDIT.source_bytes(path)
            self.assertNotIsInstance(error.exception, AUDIT.SourceBudgetError)
            self.assertIn('exit 1', str(error.exception))
            self.assertIn('corrupt frame', str(error.exception))
            with patch.object(AUDIT, 'ZSTD_BINARY', None), patch.object(AUDIT, 'run_renderer') as run, \
                 self.assertRaises(ValueError) as error:
                AUDIT.source_bytes(path)
            run.assert_not_called()
            self.assertNotIsInstance(error.exception, AUDIT.SourceBudgetError)
            self.assertIn('unavailable at run start', str(error.exception))

    def test_source_budget_record_is_uncovered_and_keeps_typed_context(self):
        with patch.object(AUDIT, 'source_bytes', side_effect=AUDIT.SourceBudgetError('source.zst: exit 124: timed out')):
            record, _ = AUDIT.inspect(('source.zst', [{'id': 'one'}]), argparse.Namespace())
        self.assertEqual(record['status'], 'uncovered')
        self.assertEqual(record['execution'], 'budget')
        self.assertEqual(record['errorType'], 'SourceBudgetError')
        self.assertEqual(record['error'], 'source.zst: exit 124: timed out')

    def test_possible_external_context_includes_inline_conditions(self):
        for source in [b'.so man1/target.1\n', b"'mso package.tmac\n", b'.soquiet absent\n',
                       b'.if 1 .so man1/target.1\n', b'.ie n .mso package\n',
                       b'.el .soquiet absent\n', b"'if t 'so target\n",
                       b'.if 1 .if 0 .so target\n', b'.if 0 .so never-executed\n',
                       b'.if \\n[register] .so target\n']:
            with self.subTest(source=source):
                self.assertIsNotNone(AUDIT.EXTERNAL.search(source))
                record = self.inspect(source=source)
                self.assertEqual(record['status'], 'uncovered')
                self.assertTrue(record['externalContext'])
                self.assertIn('possible-external-source-context', record['reason'])

    def test_external_context_does_not_match_prose_comments_or_other_requests(self):
        for source in [b'Prose mentions .so target\n', b'.\\" .so commented-out\n',
                       b'.\\" .if 1 .so comment\n', b'.B .so\n', b'.source path\n',
                       b'.soquietly absent\n', b'.if 1 .source path\n',
                       b'.if 1 .B "mention.so path"\n', b'.if 1 .B "quoted .so text"\n',
                       b'.if 1 BODY\n']:
            with self.subTest(source=source):
                self.assertIsNone(AUDIT.EXTERNAL.search(source))

    def test_decoder_identity_is_in_summary_and_rechecked_on_completion(self):
        for state in ('stable', 'changed', 'removed', 'unavailable'):
            with self.subTest(state=state), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                mant, reference, decoder = (root / name for name in ('mant', 'mandoc', 'zstd'))
                for binary in (mant, reference, decoder):
                    binary.write_bytes(b'initial binary identity')
                source = root / 'source'
                source.write_bytes(b'body')
                manifest = root / 'manifest'
                manifest.write_text(json.dumps({'id': 'one', 'source_path': str(source)}) + '\n')
                output = root / 'audit'
                output.mkdir()
                args = argparse.Namespace(mant=mant, reference=reference, manifest=manifest, max_pages=None,
                    reference_id='fixed-test', output=output, workers=1, artifact_pages=0,
                    width=80, timeout=1, verify=False)
                def inspect(_item, _args):
                    if state == 'changed':
                        decoder.write_bytes(b'changed binary identity')
                    elif state == 'removed':
                        decoder.unlink()
                    return {'sourcePath': str(source), 'identities': [{'id': 'one'}], 'status': 'clean'}, {}
                report = {}
                with patch.object(AUDIT, 'ZSTD_BINARY', None if state == 'unavailable' else decoder), \
                     patch.object(AUDIT, 'inspect', side_effect=inspect), \
                     patch.object(AUDIT.subprocess, 'check_output', side_effect=['producer', '']), \
                     patch('builtins.print'):
                    code = AUDIT.census(args, report)
                saved = json.loads((output / 'summary.json').read_text())
                self.assertEqual(saved['sourceDecoders']['zstd']['available'], state != 'unavailable')
                if state != 'unavailable':
                    self.assertEqual(saved['binaries']['zstd']['path'], str(decoder))
                    self.assertEqual(saved['binaries']['zstd']['sha256'], AUDIT.digest(b'initial binary identity'))
                else:
                    self.assertNotIn('zstd', saved['binaries'])
                self.assertEqual(saved['binariesUnchanged'], state in ('stable', 'unavailable'))
                self.assertEqual(saved['coverageComplete'], state in ('stable', 'unavailable'))
                self.assertEqual(code, int(state in ('changed', 'removed')))
                self.assertTrue(any('not an end-to-end' in note for note in saved['limitations']))

    def test_manifest_snapshot_duplicate_ids_and_empty_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest"
            args = argparse.Namespace(manifest=path, max_pages=None)
            for contents in ("", '{"id":"same","source_path":"x"}\n' * 2):
                path.write_text(contents, encoding="utf-8")
                with self.assertRaises(ValueError):
                    AUDIT.census_inputs(args)
            snapshot = b'{"id":"one","source_path":"x"}\n'
            path.write_bytes(snapshot)
            inputs = AUDIT.census_inputs(args)
            path.write_bytes(b"changed after acquisition")
            self.assertEqual(args._manifest_sha256, AUDIT.digest(snapshot))
            self.assertEqual(inputs[0][1][0]["id"], "one")

    def test_normal_setup_failure_writes_non_green_summary(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "report"
            args = ["audit", "--reference", str(Path(directory) / "absent"), "--reference-id", "test",
                    "--mant", str(Path(directory) / "absent"), "--output", str(output)]
            with patch.object(sys, "argv", args):
                self.assertEqual(AUDIT.main(), 1)
            report = json.loads((output / "summary.json").read_text())
            self.assertEqual(report["status"], "audit-error")
            self.assertFalse(report["coverageComplete"])
            self.assertEqual(report["auditError"]["type"], "FileNotFoundError")

    def test_nonfinite_timeout_is_rejected_before_launch(self):
        for value in ("nan", "inf", "-inf"):
            with patch.object(sys, "argv", ["audit", "--reference", "x", "--reference-id", "test",
                                           "--output", "unused", "--timeout=" + value]), \
                 self.assertRaises(SystemExit) as error:
                AUDIT.main()
            self.assertEqual(error.exception.code, 2)

    def test_all_bounded_findings_survive_the_census_record(self):
        original = {'findings': [{'kind': 'missing-occurrence', 'token': str(i)} for i in range(128)],
                    'coverage': {'complete': False, 'reasons': ['finding-retention-budget']}}
        kept = AUDIT.compact(original)
        self.assertEqual(kept['findingsRetained'], 128)
        self.assertEqual(kept['findings'], original['findings'])
        self.assertFalse(kept['coverage']['complete'])

    def test_plain_output_rejects_sgr_as_well_as_other_controls(self):
        self.assertEqual(plain_output_controls('中\tTEXT\n')['status'], 'covered')
        for text in ['\x1b[31mTEXT\x1b[0m', '\x1b]52;abc\x07', 'TEXT\b', 'TEXT\r']:
            self.assertEqual(plain_output_controls(text)['status'], 'hard-failure')

    def test_explaining_separator_never_clears_geometry_or_raw_status(self):
        record = {'status': 'review', 'content': {'status': 'review'}, 'geometry': {'status': 'review'}}
        result = classify(record, {'status': 'covered', 'counts': {}})
        self.assertEqual(result['category'], 'geometry-difference')
        self.assertEqual(record['status'], 'review')
        self.assertFalse(result['confirmedProductDefect'])

    def test_incomplete_frame_and_reference_controls_are_coverage_not_content_loss(self):
        frame_limited = {
            'status': 'review', 'content': {'status': 'review', 'findings': []},
            'geometry': {'status': 'covered'},
            'frames': {'reference': {'status': 'partial'}, 'mant': {'status': 'covered'}},
        }
        self.assertEqual(
            classify(frame_limited, {'status': 'review', 'counts': {'missing-occurrence': 1}})['category'],
            'frame-limited-content-coverage',
        )
        bad_reference = {
            'status': 'review',
            'content': {'status': 'review', 'findings': [{'kind': 'reference-control'}]},
            'geometry': {'status': 'covered'},
        }
        self.assertEqual(
            classify(bad_reference, {'status': 'review', 'counts': {'missing-occurrence': 1}})['category'],
            'reference-output-coverage',
        )

    def test_ambiguous_control_operand_is_not_ranked_as_a_leak(self):
        record = {'status': 'review', 'content': {'status': 'review'}, 'geometry': {'status': 'covered'}}
        self.assertEqual(
            classify(record, {'status': 'review', 'counts': {'ambiguous-control-operand': 1}})['category'],
            'ambiguous-operand-origin',
        )

    def test_source_consistent_presentation_reduction_keeps_a_distinct_residual_queue(self):
        record = {
            'status': 'review',
            'content': {'status': 'review', 'counts': {'missing-occurrence': 10}},
            'contentAssessment': {'explanations': [{'rule': 'source-consistent-example'}]},
            'geometry': {'status': 'covered'},
        }
        self.assertEqual(
            classify(record, {'status': 'review', 'counts': {'missing-occurrence': 2}})['category'],
            'mixed-presentation-and-content-review',
        )

    def test_artifact_selection_prefers_late_high_risk_and_distinct_corpora(self):
        def row(identity, priority):
            return {'status': 'review', 'identities': [{'id': identity}], 'triage': {'category': 'unexplained-content', 'priority': priority}}
        pool = ArtifactSelection(2, 8)
        pool.consider(0, row('a:first', 10), {'stdout': b'abcd'})
        pool.consider(1, row('a:duplicate', 10), {'stdout': b'abcd'})
        pool.consider(2, row('b:second', 20), {'stdout': b'abcd'})
        pool.consider(3, row('c:late-high-risk', 90), {'stdout': b'abcd'})
        self.assertEqual([i['ordinal'] for i in pool.selected()], [3, 2])
        self.assertEqual(pool.bytes, 8)
        pool.consider(4, row('d:oversized', 100), {'stdout': b'0123456789'})
        self.assertEqual(pool.bytes, 8)
        self.assertEqual(pool.omitted['single-artifact-byte-budget'], 1)

    def test_zero_artifact_budget_does_not_invent_saved_evidence(self):
        pool = ArtifactSelection(0)
        pool.consider(0, {'status': 'review'}, {'stdout': b'text'})
        self.assertEqual(pool.selected(), [])
        self.assertEqual(pool.bytes, 0)


if __name__ == "__main__":
    unittest.main()
