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

    def test_missing_redirect_target_is_external_coverage_for_all_profiles(self):
        finding = AUDIT.LEGACY['structure'].Finding(
            '/fixture/alias.1', 'hard-failure', [],
            "/fixture/alias.1: could not resolve manual .so target 'man1/target.1'",
        )
        row = {'sourcePath': '/fixture/alias.1', 'externalContext': True, 'dimensions': {}}
        for name in AUDIT.PROFILES:
            gap = AUDIT.profile_coverage_gap(name, row, finding)
            self.assertEqual(gap['status'], 'uncovered')
            self.assertEqual(gap['coverage'], 'partial-external-context')

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

    def test_renderer_failures_distinguish_missing_external_alias_from_reference_gap(self):
        missing = AUDIT.renderer_coverage_gap(
            "mant: could not load manual: could not resolve manual .so target 'man1/target.1'", True,
        )
        self.assertEqual(missing['coverage'], 'partial-external-context')
        self.assertIsNone(AUDIT.renderer_coverage_gap('ordinary ManT parser failure', False))
        reference = AUDIT.renderer_coverage_gap('troff: input stack limit exceeded', False, reference=True)
        self.assertEqual(reference['status'], 'uncovered')
        self.assertEqual(reference['coverage'], 'partial-reference-renderer')

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

    def test_cross_reference_presentation_divergence_preserves_raw_review(self):
        covered = {'execution': 'success', 'coverage': 'legacy-dimensions-covered'}
        result = {
            'fidelity-mandoc': {**covered, 'status': 'clean', 'finding': {'status': 'clean'}},
            'fidelity-groff': {**covered, 'status': 'review', 'finding': {'status': 'review'}},
        }
        AUDIT.classify_cross_reference_presentation(result)
        groff = result['fidelity-groff']
        self.assertEqual(groff['status'], 'explained')
        self.assertEqual(groff['rawStatus'], 'review')
        self.assertEqual(groff['triage'], 'cross-reference-presentation-divergence')
        self.assertEqual(groff['peerReference'], 'mandoc')
        self.assertEqual(groff['finding']['status'], 'review')

    def test_cross_reference_classifier_keeps_two_sided_or_partial_reviews_open(self):
        covered = {'execution': 'success', 'coverage': 'legacy-dimensions-covered'}
        two_sided = {
            'fidelity-mandoc': {**covered, 'status': 'review', 'finding': {'status': 'review'}},
            'fidelity-groff': {**covered, 'status': 'review', 'finding': {'status': 'review'}},
        }
        AUDIT.classify_cross_reference_presentation(two_sided)
        self.assertEqual(two_sided['fidelity-mandoc']['status'], 'review')
        self.assertEqual(two_sided['fidelity-groff']['status'], 'review')

        partial = {
            'fidelity-mandoc': {**covered, 'status': 'clean', 'finding': {'status': 'clean'}},
            'fidelity-groff': {
                'execution': 'success', 'coverage': 'partial-external-context',
                'status': 'review', 'finding': {'status': 'review'},
            },
        }
        AUDIT.classify_cross_reference_presentation(partial)
        self.assertEqual(partial['fidelity-groff']['status'], 'review')

    def test_source_proven_presentation_preserves_raw_review(self):
        result = {
            'fidelity-mandoc': {
                'execution': 'success',
                'coverage': 'legacy-dimensions-covered',
                'status': 'review',
                'finding': {'status': 'review'},
                'sourceContentAssessment': {
                    'status': 'explained',
                    'rawStatus': 'review',
                    'residualStatus': 'covered',
                    'sourceConsistentCompatibilityApplied': True,
                },
            },
        }
        AUDIT.classify_source_proven_presentation(result)
        value = result['fidelity-mandoc']
        self.assertEqual(value['status'], 'explained')
        self.assertEqual(value['rawStatus'], 'review')
        self.assertEqual(value['triage'], 'source-proven-reference-presentation')
        self.assertEqual(value['finding']['status'], 'review')

    def test_source_proven_presentation_requires_a_complete_residual(self):
        result = {
            'fidelity-groff': {
                'execution': 'success',
                'coverage': 'legacy-dimensions-covered',
                'status': 'review',
                'finding': {'status': 'review'},
                'sourceContentAssessment': {
                    'status': 'explained',
                    'rawStatus': 'review',
                    'residualStatus': 'review',
                    'sourceConsistentCompatibilityApplied': True,
                },
            },
        }
        AUDIT.classify_source_proven_presentation(result)
        self.assertEqual(result['fidelity-groff']['status'], 'review')

    def test_source_proven_named_glyph_reconciles_legacy_ascii_tokenization(self):
        # The legacy word tokenizer intentionally splits a Unicode word such
        # as Doleček.  Its independent residual comparator is consequently
        # more conservative, but an exact source/reference/Mant inventory can
        # still prove that this one legacy missing token is reference-only.
        result = {
            'fidelity-mandoc': {
                'execution': 'success',
                'coverage': 'legacy-dimensions-covered',
                'status': 'review',
                'finding': {
                    'status': 'review',
                    'missing_tokens': ['Doleek'],
                    'broken_phrases': [],
                    'signatures': [],
                },
                'sourceContentAssessment': {
                    'status': 'review',
                    'rawStatus': 'review',
                    'residualStatus': 'review',
                    'sourceConsistentCompatibilityApplied': True,
                    'explanations': [{
                        'rule': 'source-consistent-groff-named-character/v1',
                        'referenceSpellings': ['Doleek'],
                    }],
                },
            },
        }
        AUDIT.classify_source_proven_presentation(result)
        value = result['fidelity-mandoc']
        self.assertEqual(value['status'], 'explained')
        self.assertEqual(value['triage'], 'source-proven-reference-glyph-compatibility')

    def test_source_proven_default_composite_reconciles_legacy_ascii_tokenization(self):
        result = {
            'fidelity-mandoc': {
                'execution': 'success',
                'coverage': 'legacy-dimensions-covered',
                'status': 'review',
                'finding': {
                    'status': 'review',
                    'missing_tokens': ['renabled'],
                    'broken_phrases': [],
                    'signatures': [],
                },
                'sourceContentAssessment': {
                    'status': 'review',
                    'rawStatus': 'review',
                    'residualStatus': 'review',
                    'sourceConsistentCompatibilityApplied': True,
                    'explanations': [{
                        'rule': 'source-consistent-groff-default-composite/v1',
                        'referenceSpellings': ['renabled'],
                    }],
                },
            },
        }
        AUDIT.classify_source_proven_presentation(result)
        self.assertEqual(result['fidelity-mandoc']['status'], 'explained')

    def test_source_proven_eqn_subscript_reconciles_terminal_spacing(self):
        result = {
            'fidelity-mandoc': {
                'execution': 'success',
                'coverage': 'legacy-dimensions-covered',
                'status': 'review',
                'finding': {
                    'status': 'review',
                    'missing_tokens': ['log_2'],
                    'broken_phrases': [],
                    'signatures': [],
                },
                'sourceContentAssessment': {
                    'status': 'review',
                    'rawStatus': 'review',
                    'residualStatus': 'review',
                    'sourceConsistentCompatibilityApplied': True,
                    'explanations': [{
                        'rule': 'source-consistent-eqn-subscript-spacing/v1',
                        'referenceSpellings': ['log_2'],
                    }],
                },
            },
        }
        AUDIT.classify_source_proven_presentation(result)
        self.assertEqual(result['fidelity-mandoc']['status'], 'explained')

    def test_named_glyph_proof_cannot_hide_another_legacy_candidate(self):
        result = {
            'fidelity-mandoc': {
                'execution': 'success',
                'coverage': 'legacy-dimensions-covered',
                'status': 'review',
                'finding': {
                    'status': 'review',
                    'missing_tokens': ['Doleek', 'lost'],
                    'broken_phrases': [],
                    'signatures': [],
                },
                'sourceContentAssessment': {
                    'status': 'review',
                    'rawStatus': 'review',
                    'residualStatus': 'review',
                    'sourceConsistentCompatibilityApplied': True,
                    'explanations': [{
                        'rule': 'source-consistent-groff-named-character/v1',
                        'referenceSpellings': ['Doleek'],
                    }],
                },
            },
        }
        AUDIT.classify_source_proven_presentation(result)
        self.assertEqual(result['fidelity-mandoc']['status'], 'review')

    def test_completed_summary_counts_cross_reference_triage_separately(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source.1'
            source.write_bytes(b'.TH T 1\n')
            manifest = root / 'manifest.jsonl'
            manifest.write_bytes(b'{}\n')
            args = argparse.Namespace(output=root / 'audit', workers=1, batch_size=1,
                                      max_result_bytes=100000, manifest=manifest)
            report = {
                'physicalPages': 1, 'logicalPages': 1, 'rules': {}, 'binaries': {},
                'manifestSha256': AUDIT.SOURCES.file_hash(manifest),
            }

            def inspect(_item, _args):
                dimensions = {
                    name: {
                        'execution': 'success', 'status': 'clean',
                        'coverage': 'legacy-dimensions-covered',
                    }
                    for name in AUDIT.DIMENSIONS
                }
                dimensions['fidelity-groff'].update(
                    status='explained', rawStatus='review',
                    triage='cross-reference-presentation-divergence',
                )
                digest = AUDIT.SOURCES.file_hash(source)
                return {
                    'sourcePath': str(source), 'identities': [{'id': 'one'}],
                    'sourceSha256': digest, 'transportSha256': digest,
                    'dimensions': dimensions,
                }

            with patch.object(AUDIT, 'inspect_source', side_effect=inspect):
                self.assertEqual(AUDIT.execute(args, [(source, [{'id': 'one'}])], report), 0)
            summary = json.loads((args.output / 'summary.json').read_text())
            self.assertEqual(summary['dimensionCounts']['fidelity-groff']['status:explained'], 1)
            self.assertEqual(
                summary['triageCounts']['cross-reference-presentation-divergence'], 1
            )

    def test_bounded_comparison_worker_matches_the_direct_legacy_oracle(self):
        payload = {'label': 'one', 'source': '.TH T 1\n.SH BODY\nAlpha beta gamma delta\n',
                   'reference': 'Alpha beta gamma delta', 'mant': 'Alpha beta delta', 'kind': 'mandoc'}
        expected = AUDIT.FIDELITY.compare_rendered('one', payload['source'].encode(),
            payload['reference'], payload['mant'], payload['kind'])
        code, output, error = AUDIT.run_renderer([sys.executable, str(Path(AUDIT.__file__)), '--compare-worker'],
            10, AUDIT.reference_environment(), json.dumps(payload).encode(), binary_output=True)
        self.assertEqual(code, 0, error)
        worker = json.loads(output)
        self.assertEqual(
            {key: value for key, value in worker.items() if key != '_sourceContentAssessment'},
            asdict(expected.finding),
        )
        self.assertEqual(worker['_sourceContentAssessment']['rawStatus'], 'review')

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
