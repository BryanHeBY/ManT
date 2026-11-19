"""Carry native uncertainty into the public structure report and source ledger."""

import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts.roff.audit import audit_roff_structure as audit


class StructureSourceReviewTests(unittest.TestCase):
    def test_source_review_and_unrelated_violations_are_reported_separately(self):
        # CVS term_word() emits zero-width output for the unknown named
        # special in mixed/unknown. A real ONE label still owes its link.
        header = (
            '.Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n'
            '.Nd sample\n.Sh DESCRIPTION\n'
        )
        unknown = '.No \\z\n.Sx \\[foo]\n.No Y\n'
        destination = '.Sh ONE\nBODY-A.\n'
        sources = {
            'empty': header + '.Sx \\&\n' + destination,
            'visible': header + '.Sx ONE\n' + destination,
            'unknown': header + unknown + destination,
            'mixed': header + unknown + '.Sx ONE\n' + destination,
            'alias': '.so man1/mixed.1\n',
        }
        topology = dict(lists=[], tableRows=[], equations=[])
        lost_link = 'section-link at 11:2: expected "ONE" in source order'

        def transport(_bin, requests, _timeout):
            responses = {}
            for request_id, request in requests.items():
                name = Path(request['path']).stem
                uncertain = name in ('alias', 'mixed', 'unknown')
                responses[request_id] = {
                    'schema': audit.PROFILE_SCHEMA,
                    'expected': {},
                    'observed': {},
                    'topology': {'expected': topology, 'observed': topology},
                    'alias': name == 'alias',
                    'ordinalCasesForReview': [],
                    'sourceLinkOrigins': {'sectionLabelsForReview': [[9, 2]] if uncertain else []},
                    'literalDisplaysForReview': [],
                    # Simulate a separate lowering mutation in the mixed page.
                    'violations': [lost_link] if name == 'mixed' else [],
                }
            return responses

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            man = root / 'man1'
            man.mkdir()
            for name, source in sources.items():
                (man / f'{name}.1').write_text(source, encoding='utf-8')
            profiler = root / 'profiler'
            profiler.touch()
            ledger = root / 'structure.csv'
            report = root / 'structure.json'
            output = io.StringIO()
            with patch.object(audit, 'run_profile_batch', side_effect=transport), contextlib.redirect_stdout(output):
                self.assertEqual(audit.main([
                    '--manpath', str(root), '--profiler', str(profiler),
                    '--audit-db', str(ledger), '--json', str(report),
                ]), 0)
            results = json.loads(report.read_text(encoding='utf-8'))
            by_name = {Path(finding['path']).stem: finding for finding in results['findings']}
            rows = {Path(row.path).stem: row for row in audit.read_structure_database(ledger).values()}

        self.assertEqual(results['summary'], {'review': 2, 'clean': 3})
        self.assertEqual(results['source_review_occurrences'], 2)
        self.assertIn('source 9:2:', output.getvalue())
        for name in ('empty', 'visible', 'alias'):
            self.assertEqual(by_name[name]['source_review'], [])
            self.assertEqual(rows[name].scan_status, 'clean')
            self.assertEqual(rows[name].review_status, 'not-required')
        for name in ('unknown', 'mixed'):
            observation, = by_name[name]['source_review']
            self.assertEqual((observation['line'], observation['column']), (9, 2))
            self.assertIn('invalid roff escape', observation['reason'])
            self.assertEqual(rows[name].scan_status, 'review')
            self.assertEqual(rows[name].review_status, 'pending')
        self.assertEqual(by_name['mixed']['violations'], [lost_link])
        self.assertEqual(by_name['unknown']['violations'], [])

    def test_malformed_source_positions_do_not_count_as_clean(self):
        path = Path('/fixture/probe.1')
        topology = dict(lists=[], tableRows=[], equations=[])
        for positions in (None, [None], [[4, -1]], [[True, 2]], [[4, 2, 3]]):
            def transport(_bin, requests, _timeout):
                return {key: {
                    'schema': audit.PROFILE_SCHEMA, 'violations': [],
                    'expected': {}, 'observed': {},
                    'topology': {'expected': topology, 'observed': topology},
                    'ordinalCasesForReview': [],
                    'sourceLinkOrigins': {'sectionLabelsForReview': positions},
                    'literalDisplaysForReview': [],
                } for key in requests}

            with self.subTest(positions=positions):
                finding, = audit.profile_findings(
                    [path], [], 1, Path('unused'), exact_roots={path: path.parent}, batch_runner=transport,
                )
                self.assertEqual(finding.status, 'hard-failure')
