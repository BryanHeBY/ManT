"""Exact device cells stay separate from authored Unicode and omission facts."""

import gzip
import json
import unittest

from scripts.roff.fixtures import field_projection_policies as fields
from scripts.roff.fixtures import replay_rule_boundaries as replay


class RuleProjectionCards(unittest.TestCase):
    def test_all_cards_keep_complete_source_identity_and_pristine_binding(self):
        sources = {case['id']: case for case in replay.assemble()}
        devices = replay.load_bound_cards(replay.PROJECTION_CARDS)
        self.assertEqual(len(devices), 510)
        self.assertEqual(sum(bool(card['native_replacements']) for card in devices.values()), 414)
        for identity, card in devices.items():
            self.assertEqual(card['id'], identity)
            self.assertEqual(card['source_sha256'], replay.sha(sources[identity]['source']))
            self.assertEqual(card['type'], 'device-projection')
            self.assertTrue(card['rules'])
            self.assertFalse(card['native_row_joins'])
            self.assertFalse(card['product_table_seams'])
            for key in ('oracle_sha256', 'native_utf8_sha256', 'native_tree_sha256'):
                self.assertEqual(len(card[key]), 64)
        integrity = replay.load_bound_cards(replay.INTEGRITY_CARDS)
        self.assertEqual(set(integrity), {'coverage-control-source-spelling-fallback',
                         'coverage-table-source-fragment-255', 'coverage-table-source-fragment-256'})
        for identity, card in integrity.items():
            self.assertEqual(card['source_sha256'], replay.sha(sources[identity]['source']))
            self.assertFalse(sources[identity]['metadata']['native_guard_expected'])

    def test_mixed_blank_card_edits_generated_body_cells_without_erasing_authored_nbsp(self):
        cards = replay.load_bound_cards(replay.PROJECTION_CARDS)
        for identity, count in [('review-rule-02013', 1), ('review-rule-02206', 2)]:
            card = cards[identity]
            self.assertEqual(len(card['native_replacements']), count)
            # Classifier mutation, not replacement roff gold: add an authored
            # NBSP immediately after the exact generated coordinates. It must
            # survive the adapter and remain observable to all three axes.
            changes = card['native_replacements']
            width = max(change['column'] for change in changes) + 2
            row = ['X'] * width
            for change in changes:
                row[change['column']] = '\u00a0'
            row[-1] = '\u00a0'
            region = {'status': 'asserted', 'rows': [''.join(row)]}
            projected, _ = fields.project_regions(region, region, card)
            self.assertTrue(projected['rows'][0].endswith('\u00a0'))
            self.assertEqual(len(projected['rows']), 1)
            self.assertEqual(projected['rows'][0].count('\u00a0'), 1)

    def test_duplicate_card_identity_is_rejected(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'cards.json.gz'
            path.write_bytes(gzip.compress(json.dumps([{'id': 'duplicate'}] * 2).encode()))
            with self.assertRaisesRegex(ValueError, 'duplicate bound card'):
                replay.load_bound_cards(path)


if __name__ == '__main__':
    unittest.main()
