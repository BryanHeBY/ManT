"""The finite audit preserves native bytes, ordered coverage and row identity."""
from base64 import b64encode
from collections import Counter
import copy
import unittest

from scripts.roff.audit.word_row_matrix.compare import (
    UnsupportedExtraction, device_row, group_witness, native_prose, product_prose,
)
from scripts.roff.audit.word_row_matrix.manifest import MEMBERS, records
from scripts.roff.audit.word_row_matrix.oracle import (
    BASE_PROFILES, EXTRA_PROFILES, check_checkpoint, stream_bytes,
)
from scripts.roff.audit.word_row_matrix.product import public_state


class WordRowMatrixTests(unittest.TestCase):
    def test_frozen_source_language_retains_every_cohort_coordinate(self):
        counts = Counter()
        core = set()
        for block, _, source in records():
            counts[block] += 1
            if block in ("A-atomic", "A-phase", "A-rearm", "A-seam", "B-core", "C-core"):
                core.add(source)
        self.assertEqual(counts, MEMBERS)
        self.assertEqual(sum(counts.values()), 335908)
        self.assertEqual(len(core), 302268)
        self.assertEqual(len(set(BASE_PROFILES + EXTRA_PROFILES)), 11)

    def test_overstrike_decode_does_not_trim_or_split_a_wide_cell(self):
        self.assertEqual(device_row("     X \bY界e\u0301 "), ("     XY界e\u0301 ", [(7, 6)]))
        with self.assertRaises(UnsupportedExtraction):
            device_row("界\bX")

    def test_known_furniture_spans_do_not_remove_an_authored_blank_or_empty_row(self):
        heading = "".join(c + "\b" + c for c in "DESCRIPTION")
        raw = ("CHECK(1)\n\n" + heading + "\n"
               "     ALPHAX\n\n      BETA\n\n\n"
               "Linux example November 10, 2026 CHECK(1)\n")
        receipt = {"code": 0, "stdout": {"utf8": True, "base64": b64encode(raw.encode()).decode()}}
        tree = b"Sh (block) *7:2\n  Sh (head) 7:2\n      DESCRIPTION (text) 7:5\n"
        expected = native_prose(receipt, tree)
        self.assertEqual(expected["rows"], ["ALPHAX", "", " BETA", ""])
        first, last = expected["furnitureSpans"]
        self.assertEqual(raw.encode()[first[1]:last[0]], b"     ALPHAX\n\n      BETA\n\n")
        self.assertEqual(product_prose("page\nDESCRIPTION\nALPHAX\n\n BETA\n\n"), expected["rows"])

    def test_exact_public_comparison_detects_each_content_mutation(self):
        original = dict(plain="page\nDESCRIPTION\nALPHAX\n\n BETA",
                        jsonEquivalent=True, validDocument=True, ansiEquivalent=True)
        expected = {"rows": ["ALPHAX", "", " BETA"]}
        self.assertEqual(public_state(original, expected)[0], "exact")
        for before, after in ((" BETA", "BETA"), (" BETA", "  BETA"), ("BETA", "BET"),
                              ("\n\n BETA", "\n BETA"), ("ALPHAX", "ALPHAXALPHAX")):
            mutant = copy.copy(original)
            mutant["plain"] = mutant["plain"].replace(before, after, 1)
            self.assertEqual(public_state(mutant, expected)[0], "strict-row-difference")

    def test_group_witness_uses_preorder_not_source_coordinates(self):
        tree = (b"SH (block) *2:2\n  SH (head) 2:2\n      DESCRIPTION (text) 2:5\n"
                b"  SH (body) 2:2\n      ce (elem) *3:2\n          2 (text) 3:5\n"
                b"           A (text) *8:1\n          B (text) *8:1\n"
                b"      rj (elem) *9:2\n          0 (text) 9:5\n")
        groups = group_witness(tree)
        self.assertEqual([g["node"]["label"] for g in groups], ["ce", "rj"])
        self.assertEqual([g["starts"] for g in groups], [[1, 2], []])
        self.assertEqual([r["label"] for r in groups[0]["children"]], ["2", " A", "B"])
        self.assertNotEqual(groups[0]["children"][1]["path"], groups[0]["children"][2]["path"])

    def test_checkpoints_reject_missing_recipes_and_changed_raw_bytes(self):
        source = {"sourceSha256": "source", "extraWidths": False}
        catalog = [(str(index), profile) for index, profile in enumerate(BASE_PROFILES)]
        receipts = {key: {"stdout": stream_bytes(b"out"), "stderr": stream_bytes(b"")}
                    for key, _ in catalog}
        check_checkpoint(source, dict(sourceSha256="source", receipts=receipts), catalog)
        receipts["0"]["stdout"]["base64"] = b64encode(b"different").decode()
        with self.assertRaisesRegex(ValueError, "cached stdout bytes changed"):
            check_checkpoint(source, dict(sourceSha256="source", receipts=receipts), catalog)
        del receipts["0"]
        with self.assertRaisesRegex(ValueError, "every planned invocation"):
            check_checkpoint(source, dict(sourceSha256="source", receipts=receipts), catalog)


if __name__ == "__main__":
    unittest.main()
