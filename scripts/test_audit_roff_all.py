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

    def test_semantic_standalone_redirect_is_external_coverage_not_lowering_failure(self):
        finding = AUDIT.LEGACY['semantics'].Finding(
            '/fixture/alias.1', 'hard-failure', [],
            '/fixture/alias.1: standalone .so redirects require MANPATH discovery and cannot be followed by --input',
        )
        row = {'sourcePath': '/fixture/alias.1', 'externalContext': True, 'dimensions': {}}
        self.assertEqual(AUDIT.profile_coverage_gap('semantics', row, finding)['status'], 'uncovered')
        self.assertEqual(AUDIT.profile_coverage_gap('semantics', row, finding)['coverage'], 'partial-external-context')
        row['externalContext'] = False
        self.assertIsNone(AUDIT.profile_coverage_gap('semantics', row, finding))

    def test_empty_groff_reference_is_explicit_reference_coverage_gap(self):
        finding = {
            'status': 'hard-failure',
            'detail': 'the page cannot be classified as clean without a reference corpus',
            'reference_tokens': 0,
            'mant_tokens': 3,
        }
        gap = AUDIT.reference_coverage_gap('groff', finding, False)
        self.assertEqual(gap['execution'], 'success')
        self.assertEqual(gap['status'], 'uncovered')
        self.assertEqual(gap['coverage'], 'partial-reference-renderer')
        self.assertIsNone(AUDIT.reference_coverage_gap('mandoc', finding, False))
        self.assertIsNone(AUDIT.reference_coverage_gap('groff', {**finding, 'mant_tokens': 0}, False))

    def test_invalid_source_utf8_skips_strings_but_runs_all_native_profiles(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'generated.roff'
            path.write_bytes(b'\xff')
            row = AUDIT.inspect_source((path, [{'id': 'one'}]), argparse.Namespace())
        self.assertFalse(row['sourceValidUtf8'])
        self.assertEqual(set(row['dimensions']), set(AUDIT.DIMENSIONS) - set(AUDIT.PROFILES))
        self.assertTrue(all(value['execution'] == 'uncovered' for value in row['dimensions'].values()))
        args = argparse.Namespace(profiler_dir=Path('unused'), batch_timeout=1)
        for name, legacy in AUDIT.LEGACY.items():
            with patch.object(legacy, 'profile_findings', return_value=iter([legacy.Finding(str(path), 'clean', [])])) as profile:
                AUDIT.profile_dimension(name, [row], args)
            self.assertEqual(profile.call_args.args[0], [path])
            self.assertEqual(row['dimensions'][name]['execution'], 'success')
            self.assertEqual(row['dimensions'][name]['status'], 'clean')
            coverage = 'partial-non-utf8-source' if name == 'structure' else 'legacy-dimensions-covered'
            self.assertEqual(row['dimensions'][name]['coverage'], coverage)
        self.assertEqual(set(row['dimensions']), set(AUDIT.DIMENSIONS))

    def test_source_budget_type_not_detail_words_controls_classification(self):
        for error, execution in [(AUDIT.SOURCES.SourceBudgetError('LZMA dictionary memory limit reached'), 'budget'),
                                 (AUDIT.SOURCES.SourceBudgetError('zstd timed out'), 'budget'),
                                 (ValueError('invalid header exceeds expectations'), 'error')]:
            with patch.object(AUDIT.SOURCES, 'source_bytes', side_effect=error):
                row = AUDIT.inspect_source((Path('/source'), [{'id': 'one'}]), argparse.Namespace())
            self.assertTrue(all(value['execution'] == execution for value in row['dimensions'].values()))

    def test_plan_freezes_resolved_zstd_and_explicitly_records_absence(self):
        args = argparse.Namespace(mant=Path('/bin/mant'), mandoc=Path('/bin/mandoc'), groff=Path('/bin/groff'),
            profiler_dir=Path('/profiles'), _manifest_sha256='manifest', workers=1, batch_size=1,
            timeout=1, batch_timeout=1, max_result_bytes=4096, parallelism={'workers': 1})
        for decoder in (Path('/resolved/zstd'), None):
            with patch.object(AUDIT.SOURCES, 'census_inputs', return_value=[('/input', [{'id': 'one'}])]), \
                 patch.object(AUDIT.SOURCES, 'ZSTD_BINARY', decoder), \
                 patch.object(AUDIT, 'identity', side_effect=lambda path: {'path': str(path), 'sha256': 'bound'}), \
                 patch.object(AUDIT.SOURCES, 'file_hash', return_value='rule'), \
                 patch.object(AUDIT, 'git', return_value='commit'), \
                 patch.object(AUDIT.shutil, 'which', side_effect=lambda name:
                     {'xz': None, 'bzip2': '/resolved/bzip2'}.get(name, '/backend')):
                _, report = AUDIT.plan(args)
            self.assertEqual(report['dependencyAvailability']['zstd'], decoder is not None)
            self.assertFalse(report['dependencyAvailability']['xz'])
            self.assertNotIn('xz', report['binaries'])
            self.assertTrue(report['dependencyAvailability']['bzip2'])
            self.assertEqual(report['binaries']['bzip2'], {'path': '/resolved/bzip2', 'sha256': 'bound'})
            if decoder:
                self.assertEqual(report['binaries']['zstd'], {'path': str(decoder), 'sha256': 'bound'})
            else:
                self.assertNotIn('zstd', report['binaries'])

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
        self.assertEqual(results['fidelity-groff']['status'], 'hard-failure')
        self.assertEqual(results['fidelity-groff']['coverage'], 'uncovered')
        self.assertEqual(results['layout-groff']['execution'], 'uncovered')
        command = rendered.call_args_list[1].args[0]
        self.assertIn('-mandoc', command)
        self.assertIn('-Kutf8', command)
        self.assertNotIn('-man', command)

    def test_reference_source_budget_does_not_prevent_other_renderer(self):
        args = argparse.Namespace(groff=Path('/bin/groff'), mandoc=Path('/bin/mandoc'), mant=Path('/bin/mant'), timeout=1)
        source = b'.TH TEST 1\n.SH BODY\nAlpha beta gamma\n'
        finding = asdict(AUDIT.FIDELITY.compare_rendered('one', source, 'Alpha beta gamma', 'Alpha beta gamma', 'groff').finding)
        with patch.object(AUDIT.FIDELITY, 'reference_render_command', side_effect=AUDIT.SOURCES.SourceBudgetError('zstd timeout')), \
             patch.object(AUDIT, 'render', return_value=('Alpha beta gamma', {}, None)), \
             patch.object(AUDIT, 'run_renderer', return_value=(0, json.dumps(finding).encode(), '')):
            results, _ = AUDIT.render_dimensions(Path('/fixture/page.1'), source, args, False)
        self.assertEqual(results['fidelity-mandoc']['execution'], 'budget')
        self.assertEqual(results['layout-mandoc']['execution'], 'budget')
        self.assertEqual(results['fidelity-groff']['execution'], 'success')

    def test_dependency_mutation_invalidates_even_successful_completed_run(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / 'manifest'; manifest.write_bytes(b'manifest')
            decoder = root / 'zstd'; decoder.write_bytes(b'before')
            path = root / 'source'; path.write_bytes(b'.TH T 1\n')
            args = argparse.Namespace(output=root / 'audit', workers=1, batch_size=1,
                max_result_bytes=100000, manifest=manifest)
            report = {'physicalPages': 1, 'logicalPages': 1, 'rules': {},
                'binaries': {'zstd': AUDIT.identity(decoder)}, 'manifestSha256': AUDIT.SOURCES.file_hash(manifest)}
            def inspect(_item, _args):
                decoder.write_bytes(b'after')
                return {'sourcePath': str(path), 'identities': [{'id': 'one'}],
                    'transportSha256': AUDIT.SOURCES.file_hash(path), 'sourceSha256': AUDIT.SOURCES.file_hash(path),
                    'dimensions': {name: {'execution': 'success', 'status': 'clean', 'coverage': 'legacy-dimensions-covered'} for name in AUDIT.DIMENSIONS}}
            with patch.object(AUDIT, 'inspect_source', side_effect=inspect):
                self.assertEqual(AUDIT.execute(args, [(path, [{'id': 'one'}])], report), 1)
            self.assertEqual(report['status'], 'completed')
            self.assertFalse(report['binariesUnchanged'])
            self.assertFalse(report['coverageComplete'])

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
