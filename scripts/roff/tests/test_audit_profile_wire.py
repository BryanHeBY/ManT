"""Mutate successful profiler observations across the audit-driver boundary."""

from copy import deepcopy
from pathlib import Path
import unittest

from scripts.roff.audit import audit_roff_semantics as semantic
from scripts.roff.audit import audit_roff_structure as structure


def structure_page():
    # Selected fields from a successful probe with `.No \\o'x'` in two
    # ordinal bodies and a section reference with an invalid special escape.
    def topology(kind):
        return {'lists': [{'items': 2, 'kind': kind, 'sourceLine': 8}],
                'tableRows': [], 'equations': []}

    return {
        'schema': structure.PROFILE_SCHEMA,
        'alias': False,
        'expected': {'definitionItems': 2},
        'observed': {'genericListItems': 2},
        'topology': {'expected': topology('definition'), 'observed': topology('generic')},
        'ordinalCasesForReview': [[8, 2]],
        'sourceLinkOrigins': {'section': [[15, 2]], 'sectionLabelsForReview': [[15, 2]]},
        'violations': [],
    }


def semantic_page():
    # The production response for the same pair of overstrike bodies.
    return {
        'schema': semantic.PROFILE_SCHEMA,
        'mode': 'production',
        'entryCounts': {},
        'aliaslessGenericTermCount': 0,
        'aliaslessGenericTermSamples': [],
        'noteLikeEntryCount': 0,
        'noteLikeEntrySamples': [],
        'entries': [],
        'emptyEntries': [],
        'ordinalEntries': [],
        'ordinalDefinitions': [],
        'retainedPresentationOrdinals': [],
        'ordinalConversions': [],
        'ordinalConversionViolations': [],
        'ordinalCasesForReview': [[8, 2]],
        'valueDomainViolations': [],
        'semanticsComplete': True,
        'semanticViolations': [],
        'declarationGroups': {'sourceRuns': [], 'observedGroups': [], 'unexpectedGroups': [],
                              'ungroupedRuns': 0, 'unresolvedRuns': 0, 'invalidGroups': 0},
        'declarationGroupViolations': [],
        'violations': [],
    }


def successful_page(audit):
    page = structure_page() if audit is structure else semantic_page()
    page['ordinalCasesForReview'].clear()
    if audit is structure:
        page['sourceLinkOrigins']['sectionLabelsForReview'].clear()
    return page


class AuditProfileWireTests(unittest.TestCase):
    def finding(self, audit, response):
        root = Path('/fixture')
        page = root / 'man1/probe.1'
        profiler = root / 'unused'

        def transport(_profiler, requests, _timeout):
            self.assertEqual(len(requests), 1)
            request_id, = requests
            self.assertEqual(requests[request_id]['path'], str(page))
            return {request_id: {**response, 'id': request_id}}

        args = (1, profiler) if audit is structure else (profiler, 1)
        result, = audit.profile_findings(
            [page], [root], *args, exact_roots={page: root}, batch_runner=transport,
        )
        return result

    def test_required_source_fields_distinguish_absent_from_empty_and_review(self):
        for audit, path, position in (
            (structure, ['ordinalCasesForReview'], [8, 2]),
            (semantic, ['ordinalCasesForReview'], [8, 2]),
            (structure, ['sourceLinkOrigins', 'sectionLabelsForReview'], [15, 2]),
        ):
            for name, value, status, positions in (
                ('missing', None, 'hard-failure', []),
                ('null', None, 'hard-failure', []),
                ('invalid', [[-1, 2]], 'hard-failure', []),
                ('empty', [], 'clean', []),
                ('review', [position], 'review', [position]),
                ('repeated', [position, position], 'review', [position, position]),
            ):
                with self.subTest(audit=audit.__name__, path=path, fault=name):
                    response = successful_page(audit)
                    owner = response if len(path) == 1 else response[path[0]]
                    if name == 'missing':
                        del owner[path[-1]]
                    else:
                        owner[path[-1]] = value
                    result = self.finding(audit, response)
                    self.assertEqual(result.status, status)
                    self.assertEqual([[item.line, item.column] for item in result.source_review], positions)

    def test_missing_origins_is_a_protocol_error_even_with_other_violations(self):
        response = structure_page()
        response['violations'] = ['section-link at 16:2: missing resolved owner']
        result = self.finding(structure, response)
        self.assertEqual(result.status, 'review')
        self.assertEqual(result.violations, response['violations'])
        self.assertEqual(len(result.source_review), 2)
        for field in ('sourceLinkOrigins', 'ordinalCasesForReview'):
            broken = deepcopy(response)
            del broken[field]
            self.assertEqual(self.finding(structure, broken).status, 'hard-failure')

    def test_alias_and_error_payloads_keep_their_contract(self):
        alias = structure_page()
        alias['alias'] = True
        del alias['sourceLinkOrigins'], alias['ordinalCasesForReview']
        result = self.finding(structure, alias)
        self.assertEqual(result.status, 'clean')
        self.assertEqual(result.source_review, [])

        for audit in (structure, semantic):
            error = self.finding(audit, {'schema': audit.PROFILE_SCHEMA, 'error': 'missing target'})
            self.assertEqual(error.status, 'hard-failure')
            self.assertEqual(error.detail, 'missing target')
