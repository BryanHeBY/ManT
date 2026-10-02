"""Exact inventory and scope checks for independently reproduced source cards."""

import copy
import gzip
import hashlib
import json
from pathlib import Path
import unittest

from scripts.roff.fixtures import record_source_recovery_cards as recorder
from scripts.roff.fixtures.replay_rule_boundaries import assemble

ROOT = Path(__file__).resolve().parents[3]


class SourceRecoveryCards(unittest.TestCase):
    def test_extent_proofs_reconstruct_the_frozen_shadow_sources(self):
        generated = {case['id']: case for case in assemble(['retained-review'])}
        frozen = json.loads(gzip.decompress(recorder.FIXTURE.read_bytes()))
        cards = [card for card in frozen if 'pristine_extent' in card.get('proof', {})]
        self.assertEqual(len(cards), 94)
        for card in cards:
            case = generated[card['id']]
            fallback, source = recorder.derive_input(case, card['proof']['pristine_extent'])
            self.assertEqual(hashlib.sha256(case['source'].encode()).hexdigest(), card['source_sha256'])
            self.assertEqual(fallback, card['proof']['fallback'])
            self.assertEqual(hashlib.sha256(source.encode()).hexdigest(),
                             card['proof']['shadow_source_sha256'])
            self.assertEqual(source.count(recorder.WITNESS), 1)
            self.assertIn('style', card['uncovered'])
            self.assertIn('query', card['uncovered'])
            for mutation in ('family', 'outer', 'prefix', 'extent'):
                changed = copy.deepcopy(case)
                extent = dict(card['proof']['pristine_extent'])
                if mutation == 'family':
                    changed['family'] = 'unrelated'
                elif mutation == 'outer':
                    changed['metadata']['outer'] = 'm'
                elif mutation == 'prefix':
                    changed['source'] = changed['source'].replace('.No "A\\', '.No "Z\\')
                else:
                    extent['end'] = len(changed['source']) + 1
                with self.subTest(case=card['id'], mutation=mutation), self.assertRaises(ValueError):
                    recorder.derive_input(changed, extent)

    def test_complete_supported_numbered_and_unclosed_descriptor_are_not_source_cards(self):
        cases = assemble(['retained-review'])
        numbered = copy.deepcopy(next(case for case in cases if case['family'] == 'argument-boundary'
            and case['metadata'] == dict(family='argument-boundary', outer='N', delimiter="'",
                                        payload='X', end='complete')))
        numbered['source'] = numbered['source'].replace("\\N'X'", "\\N'65'")
        extent = dict(kind=0, escape=1, name=2, argument=4, end_argument=6, end=7)
        with self.assertRaisesRegex(ValueError, 'supported numbered'):
            recorder.derive_input(numbered, extent)
        descriptor = copy.deepcopy(next(case for case in cases if case['family'] == 'argument-boundary'
            and case['metadata']['outer'] == 'C' and case['metadata']['end'] == 'unclosed'))
        with self.assertRaisesRegex(ValueError, 'complete nonempty'):
            recorder.derive_input(descriptor, dict(kind=0, escape=1, name=2,
                                                  argument=4, end_argument=4, end=3))


if __name__ == '__main__':
    unittest.main()
