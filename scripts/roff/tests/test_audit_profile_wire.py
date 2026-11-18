"""Mutate successful profiler observations across the audit-driver boundary."""

from copy import deepcopy
import json
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


def ordinal_conversion():
    # Actual public profiler record for two ordinary native tag ordinals.
    return {
        'sourceLine': 8, 'listStyle': 'tag', 'terms': ['1.', '2.'],
        'expectedDisposition': 'recovered-ordered-list',
        'observedDispositions': ['recovered-ordered-list'],
        'observedIrPaths': ['section[1]/block[0]'],
    }


class AuditProfileWireTests(unittest.TestCase):
    def test_ordinal_recovery_requires_a_complete_native_tag_sequence(self):
        response = successful_page(semantic)
        response['ordinalConversions'] = [ordinal_conversion()]
        for style in ('tag', 'diag', 'hang', 'inset', 'ohang'):
            for terms in ([], ['1.'], ['1.', '2.']):
                for disposition in ('recovered-ordered-list', 'retained-definition-list'):
                    with self.subTest(style=style, terms=terms, disposition=disposition):
                        current = deepcopy(response)
                        record = current['ordinalConversions'][0]
                        record.update(listStyle=style, terms=terms, expectedDisposition=disposition,
                                      observedDispositions=[disposition])
                        admissible = bool(terms) and (disposition == 'retained-definition-list'
                                                     or style == 'tag' and len(terms) >= 2)
                        self.assertEqual(self.finding(semantic, current).status,
                                         'clean' if admissible else 'hard-failure')
        # A native non-tag list which wrongly became ordered is a valid
        # product finding. It must survive as Review with explicit evidence.
        record = response['ordinalConversions'][0]
        record.update(listStyle='hang', expectedDisposition='retained-definition-list')
        violation = ('mdoc -hang ordinal candidate at line 8 expected retained-definition-list '
                     'but observed ["recovered-ordered-list"]')
        response['ordinalConversionViolations'] = [violation]
        response['violations'] = [violation]
        self.assertEqual(self.finding(semantic, response).status, 'review')

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

    def test_ordinal_owner_pairs_and_explicit_mismatch_evidence(self):
        for disposition in ([], ['retained-definition-list'], ['other-list']):
            response = successful_page(semantic)
            conversion = ordinal_conversion()
            conversion['observedDispositions'] = disposition
            conversion['observedIrPaths'] = ['section[1]/block[0]'] if disposition else []
            response['ordinalConversions'] = [conversion]
            # Missing or wrong observed owners may be legitimate product
            # differences. A successful payload must include that finding.
            with self.subTest(disposition=disposition):
                self.assertEqual(self.finding(semantic, response).status, 'hard-failure')
                issue = ('mdoc -tag ordinal candidate at line 8 expected recovered-ordered-list '
                         f'but observed {json.dumps(disposition)}')
                response['ordinalConversionViolations'] = [issue]
                response['violations'] = [issue]
                result = self.finding(semantic, response)
                self.assertEqual(result.status, 'review')
                self.assertEqual(result.ordinal_conversions, [conversion])
                self.assertEqual(result.violations, [issue])
                response['violations'] = ['unrelated finding']
                self.assertEqual(self.finding(semantic, response).status, 'hard-failure')

        response = successful_page(semantic)
        response['ordinalConversions'] = [ordinal_conversion()]
        self.assertEqual(self.finding(semantic, response).status, 'clean')
        for field, values in {
            'sourceLine': [False, True, -1, None, 8.0, []],
            'listStyle': [None, [], 'unknown'],
            'terms': [None, [False], [{}]],
            'expectedDisposition': [None, [], 'other-list'],
            'observedDispositions': [None, [None], [[]], ['unknown']],
            'observedIrPaths': [None, [], [''], [False], ['one', 'two']],
        }.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    malformed = deepcopy(response)
                    malformed['ordinalConversions'][0][field] = value
                    self.assertEqual(self.finding(semantic, malformed).status, 'hard-failure')

    def test_structure_validates_each_record_in_both_observation_streams(self):
        for side in ('expected', 'observed'):
            response = successful_page(structure)
            current = response['topology'][side]
            current['tableRows'] = [{
                'tableSourceLine': 9, 'tableSourceColumn': 1, 'rowIndex': 0, 'kind': 'data',
                'cells': [{'columnSpan': 1, 'rowSpan': 1,
                           'verticalContinuation' if side == 'expected' else 'empty': False}],
            }]
            current['equations'] = [{'sourceLine': 10, 'context': 'inline', 'value': 'x'}]
            self.assertEqual(self.finding(structure, response).status, 'clean')
            for field in ('lists', 'tableRows', 'equations'):
                for record in (None, False, [], {}):
                    with self.subTest(side=side, field=field, record=record):
                        bad = deepcopy(response)
                        bad['topology'][side][field][0] = record
                        self.assertEqual(self.finding(structure, bad).status, 'hard-failure')
            for collection, field, values in (
                ('lists', 'items', [True, -1, 2.0]),
                ('lists', 'kind', ['unclassifiable', []]),
                ('lists', 'sourceLine', [False, None, -1]),
                ('equations', 'sourceLine', [True, -1]),
                ('equations', 'context', ['unknown', []]),
                ('equations', 'value', [None, False]),
                ('tableRows', 'tableSourceLine', [True, -1]),
                ('tableRows', 'tableSourceColumn', [True, -1]),
                ('tableRows', 'rowIndex', [False, -1]),
                ('tableRows', 'kind', ['unknown', {}, {'layout-rule': {'cells': ['unknown']}}]),
                ('tableRows', 'cells', [None, [False], [{'columnSpan': True, 'rowSpan': 1}]]),
            ):
                for value in values:
                    with self.subTest(side=side, collection=collection, field=field, value=value):
                        bad = deepcopy(response)
                        bad['topology'][side][collection][0][field] = value
                        self.assertEqual(self.finding(structure, bad).status, 'hard-failure')

            for row_kind in ('horizontal-rule', 'double-horizontal-rule',
                             {'layout-rule': {'cells': ['horizontal', 'double-horizontal']}}):
                current['tableRows'][0]['kind'] = row_kind
                self.assertEqual(self.finding(structure, response).status, 'clean')

    def test_uncertain_source_owner_findings_preserve_their_actual_review_evidence(self):
        # The genuine F5 HEAD-nested list is at 10:2. Its native source is
        # uncertain and its IR has no independent nested list owner.
        response = semantic_page()
        response['ordinalCasesForReview'] = [[10, 2]]
        issue = 'missing ordinal owner at 10:2 (disposition needs source review)'
        response['ordinalConversionViolations'] = [issue]
        response['violations'] = [issue]
        found = self.finding(semantic, response)
        self.assertEqual(found.status, 'review')
        self.assertEqual(found.violations, [issue])
        self.assertEqual([[r.line, r.column] for r in found.source_review], [[10, 2]])

        for field, value in (
            ('ordinalCasesForReview', []),
            ('ordinalCasesForReview', [[10, 3]]),
            ('ordinalCasesForReview', None),
            ('ordinalConversionViolations', [issue, issue]),
            ('ordinalConversionViolations', ['unbound source owner']),
            ('violations', []),
        ):
            with self.subTest(field=field, value=value):
                broken = deepcopy(response)
                broken[field] = value
                self.assertEqual(self.finding(semantic, broken).status, 'hard-failure')

        # Same-coordinate macro invocations remain separate occurrences.
        response['ordinalCasesForReview'] *= 2
        response['ordinalConversionViolations'] *= 2
        response['violations'] *= 2
        self.assertEqual(self.finding(semantic, response).status, 'review')
        response['ordinalConversions'] = [ordinal_conversion()]
        response['ordinalConversions'][0]['observedDispositions'] = ['other-list']
        confirmed = ('mdoc -tag ordinal candidate at line 8 expected recovered-ordered-list '
                     'but observed ["other-list"]')
        response['ordinalConversionViolations'].insert(0, confirmed)
        response['violations'].insert(0, confirmed)
        self.assertEqual(self.finding(semantic, response).status, 'review')
        # Rust emits the conclusive conversion mismatches before source-bound
        # owner findings. Reordering or dropping either stream is invalid.
        response['ordinalConversionViolations'].reverse()
        self.assertEqual(self.finding(semantic, response).status, 'hard-failure')
