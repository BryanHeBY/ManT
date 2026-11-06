"""Complete source identities and independent native/recovery witnesses."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-engine/tests/roff_lowering/deferred_ownership'


class DeferredOwnershipFixtureTests(unittest.TestCase):
    def test_sources_are_complete_distinct_and_bound_to_native_profiles(self):
        cases = json.loads((FIXTURE / 'cases.json').read_bytes())['cases']
        self.assertEqual(Counter(c['family'] for c in cases), {'D1': 180, 'D2': 39})
        self.assertEqual(len({c['id'] for c in cases}), 219)
        self.assertEqual(len({c['source_sha256'] for c in cases}), 219)
        self.assertEqual(Counter(c['baseline'] for c in cases if c['family'] == 'D1'),
                         {'ConfirmedFailure': 69, 'PassControl': 111})
        d2 = [case for case in cases if case['family'] == 'D2']
        self.assertEqual(sum(bool(case['reading_rows']) for case in d2), 10)
        self.assertEqual(sum(len(case['facts']['members']) for case in d2), 43)
        self.assertEqual(sum(len(case['facts']['tail_members']) for case in d2), 9)
        for case in cases:
            source = case['source'].encode()
            self.assertEqual(hashlib.sha256(source).hexdigest(), case['source_sha256'])
            self.assertIn(b'.Sh DESCRIPTION' if source.startswith(b'.Dd') else b'.SH DESCRIPTION', source)
            self.assertEqual(set(case['native']['profiles']), {'ascii', 'utf8', 'html', 'tree', 'lint'})
            for link in case['links']:
                for member in link['source']:
                    span = member['byteRange']
                    self.assertTrue(source[span['start']:span['end']])
            for recovery in case['recoveries']:
                span = recovery['source']['byteRange']
                self.assertEqual(source[span['start']:span['end']].decode(), recovery['text'])
                self.assertLess(recovery['ordinal'], recovery['occurrences'])

    def test_consumer_subset_and_actual_private_budget_sources(self):
        cases = {c['id']: c for c in json.loads((FIXTURE / 'cases.json').read_bytes())['cases']}
        consumers = json.loads((FIXTURE / 'consumers.json').read_bytes())['cases']
        self.assertEqual(len(consumers), 42)
        self.assertEqual(len({c['id'] for c in consumers}), 42)
        for case in consumers:
            self.assertEqual(case['source_sha256'], cases[case['id']]['source_sha256'])
            self.assertEqual(case['actual_buffer_widths'], [40, 78])
        budget = json.loads((ROOT / 'crates/mant-codec/src/mandoc/blocks/tables/recovery/budget_tests/fixtures/deferred-ownership.json').read_bytes())
        self.assertEqual(len(budget['cases']), 4)
        for case in budget['cases']:
            self.assertEqual(hashlib.sha256(case['source'].encode()).hexdigest(), case['source_sha256'])
            self.assertIn('.TS\n', case['source'])
            self.assertIn('OLDACCEPTED', case['source'])
            self.assertIn('AFTERLINK', case['source'])
        markers = json.loads((FIXTURE / 'markers.json').read_bytes())
        self.assertEqual(len(markers), 6)
        for case in markers:
            self.assertEqual(hashlib.sha256(case['source'].encode()).hexdigest(), case['source_sha256'])
            self.assertEqual(set(case['profiles']), {'ascii', 'utf8', 'html', 'tree', 'lint'})


class AnnotationOwnershipFixtureTests(unittest.TestCase):
    def test_complete_sources_and_independent_profile_identities(self):
        root = ROOT / 'crates/mant-codec/src/mandoc/tests/annotation_ownership'
        for name, count, profile_key in [
            ('families', 1246, 'native_profile_identity'),
            ('boundaries', 162, 'oracle_profiles'),
            ('costs', 52, 'profiles'),
        ]:
            cases = json.loads((root / (name + '.json')).read_bytes())['cases']
            self.assertEqual(len(cases), count)
            self.assertEqual(len({case['id'] for case in cases}), count)
            for case in cases:
                self.assertEqual(hashlib.sha256(case['source'].encode()).hexdigest(), case['source_sha256'])
                profiles = case[profile_key]
                self.assertEqual(set(profiles), {'ascii', 'utf8', 'html', 'tree', 'lint'} if name != 'costs' else {'utf8', 'html', 'lint'})
                for receipt in profiles.values():
                    self.assertEqual(len(receipt['stdout_sha256']), 64)
                    self.assertEqual(len(receipt['stderr_sha256']), 64)
        families = json.loads((root / 'families.json').read_bytes())['cases']
        self.assertEqual(Counter(c['family'] for c in families), {'Wrapper': 648, 'Pair': 288, 'ManInventory': 310})
        self.assertEqual(sum(c['family'] in {'Wrapper', 'Pair'} for c in families), 936)
        self.assertTrue(all(isinstance(c['unknown_members'], list) for c in families))

    def test_supplemental_source_profiles_are_permanent_and_complete(self):
        root = ROOT / 'crates/mant-codec/src/mandoc/tests/annotation_ownership'
        cases = json.loads((root / 'misc.json').read_bytes())['cases']
        self.assertEqual(len(cases), 41)
        self.assertEqual(len({c['id'] for c in cases}), len(cases))
        for case in cases:
            self.assertIn('DESCRIPTION', case['source'])
            if 'source_sha256' in case:
                self.assertEqual(hashlib.sha256(case['source'].encode()).hexdigest(), case['source_sha256'])
            profiles = case.get('profiles', case.get('oracle_profiles'))
            self.assertEqual(set(profiles), {'ascii', 'utf8', 'html', 'tree', 'lint'})
            self.assertIn('links', case['expected'])


if __name__ == '__main__':
    unittest.main()
