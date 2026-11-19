"""Exercise stale ledger selection without changing human review ownership."""

import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts.roff.audit import audit_roff_semantics as semantic
from scripts.roff.audit import audit_roff_structure as structure


class AuditProfileMigrationTests(unittest.TestCase):
    def test_ordinal_review_positions_keep_occurrences_and_reject_malformed_data(self):
        for observe, required in (
            (structure.structure_source_reviews, {
                'sourceLinkOrigins': {'sectionLabelsForReview': []}, 'literalDisplaysForReview': [],
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
            (structure, 'mant.roff-structure-profile/v12',
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
