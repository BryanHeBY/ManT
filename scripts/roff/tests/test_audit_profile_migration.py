"""Exercise stale ledger selection without changing human review ownership."""

import bz2
import contextlib
import gzip
import io
import json
import lzma
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts.roff.audit import audit_roff_semantics as semantic
from scripts.roff.audit import audit_roff_structure as structure
from scripts.roff.audit import audit_roff_layout as layout


class AuditProfileMigrationTests(unittest.TestCase):
    def test_structure_retry_rotation_survives_a_permanent_failure(self):
        for quota in ('--max-pages', '--max-pages-per-section'):
            for undecodable in (False, True):
                with self.subTest(quota=quota, undecodable=undecodable), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    man = root / 'man1'
                    man.mkdir()
                    a = man / ('a.1.gz' if undecodable else 'a.1')
                    b, c = man / 'b.1', man / 'c.1'
                    source = b'.TH TEST 1\n.SH DESCRIPTION\nOwner\n'
                    a.write_bytes(source)
                    b.write_bytes(source)
                    c.write_bytes(source)
                    profiler = root / 'profiler'
                    profiler.touch()
                    ledger = root / 'audit.csv'
                    args = ['--manpath', str(root), '--profiler', str(profiler), '--audit-db', str(ledger)]
                    failed = {a, b}
                    visits = []

                    def probe(selected, *_args):
                        for page in selected:
                            visits.append(page)
                            status = 'hard-failure' if page in failed else 'clean'
                            yield structure.Finding(structure.relative_label(page, [root]), status, [])

                    def scan(*options):
                        return structure.main([*args, *options])

                    with (patch.object(structure, 'profile_findings', side_effect=probe),
                          contextlib.redirect_stdout(io.StringIO())):
                        for _ in range(3):
                            scan(quota, '1')
                        self.assertEqual(visits, [b, c] if undecodable else [a, b, c])
                        self.assertEqual(len(structure.read_structure_database(ledger)), 2 if undecodable else 3)
                        failed.remove(b)
                        self.assertEqual(scan(quota, '1'), 1)
                        self.assertEqual(scan(quota, '1'), 0)
                        self.assertEqual(visits[-2:], [c, b] if undecodable else [a, b])
                        if undecodable:
                            a.write_bytes(gzip.compress(source))
                        failed.clear()
                        self.assertEqual(scan(), 0)
                        self.assertEqual(visits[-1], a)
                        records = structure.read_structure_database(ledger).values()
                        self.assertEqual(len(records), 3)
                        self.assertTrue(all(record.scan_status == 'clean' for record in records))
                        self.assertFalse(ledger.with_suffix('.csv.retries.json').exists())

    def test_bad_compression_is_page_local_and_never_stamps_an_identity(self):
        def zstd_compress(source):
            return subprocess.run(['zstd', '-q', '-c'], input=source, capture_output=True, check=True).stdout

        compressors = [('gz', gzip.compress), ('bz2', bz2.compress), ('xz', lzma.compress)]
        if shutil.which('zstd'):
            compressors.append(('zst', zstd_compress))
        for suffix, compress in compressors:
            for bad in ('short', 'header'):
                with self.subTest(suffix=suffix, bad=bad), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    source = b'.TH PROBE 1\n.SH DESCRIPTION\nTEXT\n'
                    healthy = root / 'a.1'
                    damaged = root / f'z.1.{suffix}'
                    healthy.write_bytes(source)
                    packed = compress(source)
                    damaged.write_bytes(packed[:-1] if bad == 'short' else b'invalid compressed input')
                    ledger = root / 'audit.csv'
                    args = ['--manpath', str(root), '--profiler', str(healthy), '--audit-db', str(ledger)]

                    def probe(selected, *_args):
                        return [structure.Finding(structure.relative_label(page, [root]), 'clean', []) for page in selected]

                    with (patch.object(structure, 'profile_findings', side_effect=probe) as profile,
                          contextlib.redirect_stdout(io.StringIO()) as output,
                          contextlib.redirect_stderr(io.StringIO()) as diagnostics):
                        self.assertEqual(structure.main([*args, '--source-pattern', 'DESCRIPTION']), 0)
                        self.assertIn(damaged.name, diagnostics.getvalue())
                        self.assertEqual(profile.call_args.args[0], [healthy])
                        self.assertEqual(structure.main([*args, '--max-pages', '1']), 1)
                        self.assertIn('could not decode source:', output.getvalue())
                        self.assertEqual([Path(row.path).name for row in structure.read_structure_database(ledger).values()],
                                         [healthy.name])
                        damaged.write_bytes(packed)
                        if suffix in {'xz', 'bz2'}:
                            self.assertEqual(structure.main([*args, '--max-pages-per-section', '1']), 1)
                            self.assertEqual(profile.call_args.args[0], [])
                            self.assertEqual(len(structure.read_structure_database(ledger)), 1)
                            continue
                        self.assertEqual(structure.main([*args, '--max-pages-per-section', '1']), 0)
                        self.assertEqual(profile.call_args.args[0], [damaged])
                        rows = list(structure.read_structure_database(ledger).values())
                        self.assertEqual([Path(row.path).name for row in rows], [healthy.name, damaged.name])
                        self.assertEqual(rows[1].digest, rows[0].digest)

    def test_unchanged_recovery_clears_the_retry_and_filtered_report_is_fresh(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            page, ledger, report = (root / name for name in ('a.1.gz', 'audit.csv', 'scan.json'))
            retry = ledger.with_suffix('.csv.retries.json')
            source = gzip.compress(b'.TH TEST 1\n.SH DESCRIPTION\nBODY\n')
            page.write_bytes(source)
            args = ['--manpath', str(root), '--profiler', str(page), '--audit-db', str(ledger), '--json', str(report)]

            def scan(selected, *_args):
                return [structure.Finding(structure.relative_label(path, [root]), 'clean', []) for path in selected]

            with patch.object(structure, 'profile_findings', side_effect=scan) as profiler:
                self.assertEqual(structure.main(args), 0)
                self.assertTrue(json.loads(report.read_text())['findings'])
                page.write_bytes(b'')
                self.assertEqual(structure.main([*args, '--source-pattern', 'BODY']), 0)
                current = json.loads(report.read_text())
                self.assertFalse(current['findings'])
                self.assertEqual(len(current['uncovered_sources']), 1)
                self.assertEqual(structure.main(args), 1)
                self.assertTrue(retry.exists())
                page.write_bytes(source)
                calls = profiler.call_count
                self.assertEqual(structure.main(args), 0)
                self.assertEqual(profiler.call_count, calls)
                self.assertFalse(retry.exists())
                self.assertFalse(json.loads(report.read_text())['uncovered_sources'])

    def test_layout_does_not_send_unverified_transport_to_the_renderer(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            page, bad, misnamed, ledger, report = (root / name for name in (
                'a.1.gz', 'b.1.gz', 'c.1', 'layout.csv', 'layout.json',
            ))
            encoded = gzip.compress(b'.TH TEST 1\n.SH DESCRIPTION\nBODY\n')
            page.write_bytes(encoded)
            bad.write_bytes(encoded[:-1])
            misnamed.write_bytes(encoded)
            args = ['--manpath', str(root), '--mant', str(page), '--audit-db', str(ledger), '--json', str(report)]

            def scan(selected, _roots, _arguments):
                self.assertEqual(selected, [page])
                label = layout.relative_label(page, [root])
                return {label: layout.Finding(label, 'clean', 'clean', [], None)}, None

            with patch.object(layout, 'run_layout_auditor', side_effect=scan) as render:
                self.assertEqual(layout.main(args), 1)
                render.assert_called_once()
                result = json.loads(report.read_text())
                self.assertEqual(result['summary'], {'clean': 1, 'hard-failure': 2})
                self.assertEqual(len(result['uncovered_sources']), 2)
                self.assertEqual(layout.main([*args, '--source-pattern', '^NEVER$']), 0)
                self.assertFalse(json.loads(report.read_text())['findings'])
                render.assert_called_once()

    def test_retry_pruning_requires_complete_enumeration_of_the_same_scope(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            other = root / 'other'
            other.mkdir()
            page = root / 'a.1.gz'
            page.write_bytes(b'truncated')
            profiler = root / 'profiler'
            profiler.touch()
            ledger = root / 'audit.csv'
            retry = ledger.with_suffix('.csv.retries.json')
            args = ['--profiler', str(profiler), '--audit-db', str(ledger)]
            same = [*args, '--manpath', str(root), '--corpus', 'first']
            self.assertEqual(structure.main(same), 1)
            before = retry.read_bytes()
            page.unlink()
            for incomplete in (['--max-pages', '1'], ['--source-pattern', 'DESCRIPTION'],
                               ['--man-section', '7'], ['--recorded-only']):
                self.assertEqual(structure.main([*same, *incomplete]), 0)
                self.assertEqual(retry.read_bytes(), before)
            self.assertEqual(structure.main([*same, '--corpus', 'second']), 0)
            self.assertEqual(structure.main([*args, '--manpath', str(other), '--corpus', 'first']), 0)
            self.assertEqual(retry.read_bytes(), before)
            self.assertEqual(structure.main([*same, '--recheck-recorded']), 0)
            self.assertFalse(retry.exists())

    def test_structure_limited_scans_cover_new_pages_before_retrying_persistent_failures(self):
        for quota in ('--max-pages', '--max-pages-per-section'):
            with self.subTest(quota=quota), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                for section in ('1', '7'):
                    directory = root / f'man{section}'
                    directory.mkdir()
                    for name in ('a', 'b'):
                        (directory / f'{name}.{section}').write_text(
                            f'.TH {name.upper()} {section}\n.SH DESCRIPTION\n{section} {name}\n',
                            encoding='utf-8',
                        )
                profiler = root / 'profiler'
                profiler.touch()
                ledger = root / 'audit.csv'
                args = ['--manpath', str(root), '--corpus', 'test', '--profiler', str(profiler),
                        '--audit-db', str(ledger), quota, '1']
                failing = True

                def probe(selected, *_args):
                    for page in selected:
                        label = structure.relative_label(page, [root])
                        status = 'hard-failure' if failing and page.stem == 'a' else 'clean'
                        yield structure.Finding(label, status, [])

                def scan():
                    with contextlib.redirect_stdout(io.StringIO()):
                        return structure.main(args)

                with patch.object(structure, 'profile_findings', side_effect=probe) as profile:
                    self.assertEqual(scan(), 1)
                    rounds = 3 if quota == '--max-pages' else 1
                    for _ in range(rounds):
                        scan()
                    snapshot = list(structure.read_structure_database(ledger).values())
                    self.assertEqual(len(snapshot), 4)
                    self.assertEqual([row.scan_status for row in snapshot].count('hard-failure'), 2)
                    self.assertEqual(scan(), 1)
                    failing = False
                    for _ in range(2):
                        self.assertEqual(scan(), 0)
                    self.assertTrue(all(row.scan_status == 'clean' for row in
                                        structure.read_structure_database(ledger).values()))
                    count = profile.call_count
                    self.assertEqual(scan(), 0)
                    self.assertEqual(profile.call_count, count)

    def test_invalid_structure_profiler_cannot_stamp_source_as_checked(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            man = root / 'man1'
            man.mkdir()
            page = man / 'probe.1'
            page.write_text('.TH TEST 1\n.SH DESCRIPTION\nFIRST\n', encoding='utf-8')
            profiler = root / 'profiler'
            profiler.touch()
            ledger = root / 'audit.csv'
            args = ['--manpath', str(root), '--corpus', 'test', '--profiler', str(profiler),
                    '--audit-db', str(ledger)]
            topology = dict(lists=[], tableRows=[], equations=[])
            response = {
                'schema': 'mant.roff-structure-profile/v24', 'expected': {}, 'observed': {},
                'violations': [], 'topology': {'expected': topology, 'observed': topology},
                'sourceLinkOrigins': {'sectionLabelsForReview': []},
                'ordinalCasesForReview': [], 'literalDisplaysForReview': [],
            }

            def exchange(_binary, requests, _timeout):
                return dict.fromkeys(requests, response)

            with patch.object(structure, 'run_profile_batch', side_effect=exchange) as run:
                for schema in (response['schema'], structure.PROFILE_SCHEMA):
                    response['schema'] = schema
                    self.assertEqual(structure.main(args), 1)
                    self.assertEqual(next(iter(structure.read_structure_database(ledger).values())).scan_status,
                                     'hard-failure')
                response['definitionHeadsForReview'] = []
                self.assertEqual(structure.main(args), 0)
                self.assertEqual(run.call_count, 3)
                self.assertEqual(structure.main(args), 0)
                self.assertEqual(run.call_count, 3)
                page.write_text('.TH TEST 1\n.SH DESCRIPTION\nSECOND\n', encoding='utf-8')
                self.assertEqual(structure.main(args), 0)
                self.assertEqual(run.call_count, 4)

    def test_ordinal_review_positions_keep_occurrences_and_reject_malformed_data(self):
        for observe, required in (
            (structure.structure_source_reviews, {
                'sourceLinkOrigins': {'sectionLabelsForReview': []}, 'literalDisplaysForReview': [],
                'definitionHeadsForReview': [],
            }),
            (semantic.ordinal_source_reviews, {}),
        ):
            positions = [[8, 2], [8, 2], [9, 2]]
            reviews = observe({**required, 'ordinalCasesForReview': positions})
            self.assertEqual([[row.line, row.column] for row in reviews], positions)
            self.assertTrue(all('unproved roff execution' in row.reason for row in reviews))
            for invalid in (None, [None], [[True, 2]], [[8, -1]], [[8, 2, 1]]):
                self.assertIsNone(observe({**required, 'ordinalCasesForReview': invalid}))

    def test_stale_profiles_are_rescanned_once_without_losing_annotations(self):
        for audit, old_schema, read, write in (
            (structure, 'mant.roff-structure-profile/v24',
             structure.read_structure_database, structure.write_structure_database),
            (semantic, 'mant.roff-semantic-profile/v13', semantic.read_database, semantic.write_database),
        ):
            with self.subTest(audit=audit.__name__), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / 'man1').mkdir()
                pages = ['man1/new-review.1', 'man1/reviewed.1']
                for page in pages:
                    (root / page).write_text(f'.TH TEST 1\n.SH DESCRIPTION\n{page}\n', encoding='utf-8')
                profiler = root / 'profiler'
                profiler.touch()
                ledger = root / 'audit.csv'
                report = root / 'report.json'
                previous = [audit.AuditRecord(
                    corpus='test', path=audit.relative_label(root / page, [root]), section='1',
                    digest=audit.source_digest(root / page),
                    profile_schema=old_schema, scan_status=scan, review_status=status, note=note,
                ) for page, scan, status, note in [
                    (pages[0], 'clean', 'not-required', ''),
                    (pages[1], 'review', 'false-positive', 'Reviewed source line 3.'),
                ]]
                write(ledger, previous)
                original = ledger.read_bytes()
                args = [
                    '--manpath', str(root), '--corpus', 'test', '--profiler', str(profiler),
                    '--audit-db', str(ledger), '--json', str(report),
                ]

                def findings(selected, *_args):
                    return [audit.Finding(audit.relative_label(page, [root]), 'review', ['source line 3'])
                            for page in selected]

                with patch.object(audit, 'profile_findings', side_effect=findings) as profile:
                    with contextlib.redirect_stdout(io.StringIO()):
                        if audit is semantic:
                            self.assertEqual(audit.main([*args, '--recorded-only', '--verify']), 1)
                            self.assertEqual(ledger.read_bytes(), original)

                        self.assertEqual(audit.main(args), 0)
                        self.assertEqual(profile.call_args.args[0], [root / page for page in pages])
                        migrated = list(read(ledger).values())
                        self.assertEqual([row.digest for row in migrated], [row.digest for row in previous])
                        self.assertEqual([row.profile_schema for row in migrated], [audit.PROFILE_SCHEMA] * 2)
                        self.assertEqual([row.review_status for row in migrated], ['pending', 'false-positive'])
                        self.assertEqual([row.note for row in migrated], ['', 'Reviewed source line 3.'])
                        output = json.loads(report.read_text(encoding='utf-8'))
                        self.assertEqual(output['summary'], {'review': 2})
                        self.assertEqual([result['violations'] for result in output['findings']], [['source line 3']] * 2)

                        after_update = ledger.read_bytes()
                        profile.reset_mock()
                        self.assertEqual(audit.main(args), 0)
                        if profile.called:
                            self.assertEqual(profile.call_args.args[0], [])
                        self.assertEqual(ledger.read_bytes(), after_update)

                        self.assertEqual(audit.main([*args, '--recorded-only']), 0)
                        self.assertEqual(profile.call_args.args[0], [root / page for page in pages])
                        self.assertEqual(ledger.read_bytes(), after_update)
