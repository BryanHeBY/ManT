"""Failure and coverage tests for manifest orchestration, not corpus acceptance."""
import argparse
from dataclasses import asdict
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from roff_audit_common import run_bounded_profile_batch

SPEC = importlib.util.spec_from_file_location('all_audit_test', Path(__file__).with_name('audit-roff-all.py'))
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class AllAuditTests(unittest.TestCase):
    def test_manifest_keeps_arbitrary_suffix_and_all_logical_ids(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'generated.roff'
            source.write_bytes(b'.TH X 1\n.SH BODY\nContent\n')
            manifest = root / 'manifest.jsonl'
            manifest.write_text('\n'.join(json.dumps({'id': key, 'source_path': str(source)}) for key in ('one', 'two')))
            args = argparse.Namespace(manifest=manifest, max_pages=None)
            inputs = AUDIT.SOURCES.census_inputs(args)
            self.assertEqual(len(inputs), 1)
            self.assertEqual(len(inputs[0][1]), 2)
            self.assertEqual(AUDIT.hierarchy(source), root)
            for text in ('', json.dumps({'id': 'one', 'source_path': str(source)}) + '\n' + json.dumps({'id': 'one', 'source_path': str(source)})):
                manifest.write_text(text)
                with self.assertRaises(ValueError):
                    AUDIT.SOURCES.census_inputs(args)

    def test_exact_roots_reach_all_four_legacy_interpreters(self):
        path = Path('/fixture/generated.roff')
        for name, legacy in AUDIT.LEGACY.items():
            requests_seen = {}
            def run(_profiler, requests, _timeout):
                requests_seen.update(requests)
                return {key: {'id': key, 'schema': legacy.PROFILE_SCHEMA, 'error': 'deliberate test failure'} for key in requests}
            findings = list(legacy.profile_findings([path], [], profiler=Path('unused'), timeout=1,
                exact_roots={path: path.parent}, batch_runner=run))
            self.assertEqual(len(requests_seen), 1, name)
            self.assertEqual(next(iter(requests_seen.values()))['path'], str(path))
            self.assertEqual(next(iter(requests_seen.values()))['root'], str(path.parent))
            self.assertEqual(findings[0].detail, 'deliberate test failure')

    def test_transport_distinguishes_budget_missing_invalid_and_crash_isolation(self):
        requests = {key: {'id': key, 'path': key} for key in ('one', 'two')}
        with patch('roff_reference.run_renderer', return_value=(124, b'', 'timed out')):
            result = run_bounded_profile_batch(Path('unused'), requests, 1)
        self.assertEqual({r['_execution'] for r in result.values()}, {'budget'})
        with patch('roff_reference.run_renderer', return_value=(0, b'{"id":"one"}\n', '')):
            result = run_bounded_profile_batch(Path('unused'), requests, 1)
        self.assertEqual(result['two']['_execution'], 'error')
        for output in (b'[]', b'\xff', b'{"id":"one"}\n{"id":"one"}'):
            with patch('roff_reference.run_renderer', return_value=(0, output, '')):
                result = run_bounded_profile_batch(Path('unused'), requests, 1)
            self.assertTrue(all(r['_execution'] == 'error' for r in result.values()))
        with patch('roff_reference.run_renderer', side_effect=[(1, b'', 'crash'),
                   (0, b'{"id":"one"}', ''), (1, b'', 'bad page')]):
            result = run_bounded_profile_batch(Path('unused'), requests, 5)
        self.assertNotIn('_execution', result['one'])
        self.assertEqual(result['two']['_execution'], 'error')

    def test_profile_budget_is_not_hidden_by_legacy_schema_failure(self):
        row = {'sourcePath': '/fixture/generated.roff', 'externalContext': False, 'dimensions': {}}
        with patch.object(AUDIT, 'run_bounded_profile_batch', side_effect=lambda _p, requests, _t:
            {key: {'id': key, '_execution': 'budget', 'error': 'budget exhausted'} for key in requests}):
            AUDIT.profile_dimension('structure', [row], argparse.Namespace(profiler_dir=Path('unused'), batch_timeout=1))
        self.assertEqual(row['dimensions']['structure']['execution'], 'budget')
        self.assertEqual(row['dimensions']['structure']['coverage'], 'uncovered')

    def test_invalid_source_utf8_covers_no_dimension(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'generated.roff'
            path.write_bytes(b'\xff')
            row = AUDIT.inspect_source((path, [{'id': 'one'}]), argparse.Namespace())
        self.assertEqual(set(row['dimensions']), set(AUDIT.DIMENSIONS))
        self.assertTrue(all(value['execution'] == 'uncovered' for value in row['dimensions'].values()))

    def test_reused_oracle_preserves_original_candidates_and_ngram_default(self):
        source = b'.TH T 1\n.SH BODY\nAlpha beta gamma delta epsilon\n'
        expected = AUDIT.FIDELITY.compare_rendered('one', source,
            'Alpha beta gamma delta epsilon', 'Alpha beta epsilon', 'mandoc')
        self.assertEqual(expected.finding.status, 'review')
        self.assertTrue(expected.finding.missing_tokens)
        value = AUDIT.bounded_finding({'status': 'review', 'candidates': ['loss'] * 100})
        self.assertEqual(value['finding']['status'], 'review')
        self.assertEqual(value['retentionTruncations'][0]['originalLength'], 100)
        self.assertEqual(len(value['finding']['candidates']), 16)

    def test_bounded_comparison_worker_matches_the_direct_legacy_oracle(self):
        payload = {'label': 'one', 'source': '.TH T 1\n.SH BODY\nAlpha beta gamma delta\n',
                   'reference': 'Alpha beta gamma delta', 'mant': 'Alpha beta delta', 'kind': 'mandoc'}
        expected = AUDIT.FIDELITY.compare_rendered('one', payload['source'].encode(),
            payload['reference'], payload['mant'], payload['kind'])
        code, output, error = AUDIT.run_renderer([sys.executable, str(Path(AUDIT.__file__)), '--compare-worker'],
            10, AUDIT.reference_environment(), json.dumps(payload).encode(), binary_output=True)
        self.assertEqual(code, 0, error)
        self.assertEqual(json.loads(output), asdict(expected.finding))

    def test_direct_groff_uses_mandoc_and_layout_missing_is_uncovered(self):
        args = argparse.Namespace(groff=Path('/bin/groff'), mandoc=Path('/bin/mandoc'), mant=Path('/bin/mant'), timeout=1)
        source = b'.Dd September 11, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd document\n'
        finding = asdict(AUDIT.FIDELITY.Finding('one', 'hard-failure'))
        with patch.object(AUDIT.FIDELITY, 'reference_render_command', return_value=(None, None, 'unsupported context')), \
             patch.object(AUDIT, 'render', return_value=('TEST', {}, None)) as rendered, \
             patch.object(AUDIT, 'run_renderer', return_value=(0, json.dumps(finding).encode(), '')):
            results, _ = AUDIT.render_dimensions(Path('/fixture/generated.roff'), source, args, False)
        self.assertEqual(results['fidelity-mandoc']['execution'], 'uncovered')
        self.assertEqual(results['layout-groff']['execution'], 'uncovered')
        command = rendered.call_args_list[1].args[0]
        self.assertIn('-mandoc', command)
        self.assertIn('-Kutf8', command)
        self.assertNotIn('-man', command)

    def test_fatal_processing_error_still_writes_incomplete_summary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / 'manifest'
            manifest.write_bytes(b'stable')
            args = argparse.Namespace(output=root / 'audit', workers=1, batch_size=1,
                                      max_result_bytes=4096, manifest=manifest)
            report = {'physicalPages': 1, 'logicalPages': 2, 'binaries': {}, 'rules': {},
                      'manifestSha256': hashlib.sha256(b'stable').hexdigest()}
            with patch.object(AUDIT, 'inspect_source', side_effect=RuntimeError('injected failure')):
                self.assertEqual(AUDIT.execute(args, [(root / 'input', [{}, {}])], report), 1)
            summary = json.loads((args.output / 'summary.json').read_text())
            self.assertEqual(summary['status'], 'incomplete')
            self.assertEqual(summary['unprocessedLogicalPages'], 2)
            self.assertFalse(summary['coverageComplete'])
            self.assertTrue(summary['reviewPending'])
            self.assertTrue(all(value['execution:uncovered'] == 2 for value in summary['dimensionCounts'].values()))


if __name__ == '__main__':
    unittest.main()
