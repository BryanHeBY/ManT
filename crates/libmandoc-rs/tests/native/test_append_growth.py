#!/usr/bin/env python3
"""Check the pristine TEXT observer independently of the candidate parser."""

import unittest

from run_append_growth import tree_texts


class TreeTextObservation(unittest.TestCase):
    def test_native_attributes_do_not_hide_text(self):
        # CVS tree.c::print_attr prints DELIMO/NODE_LINE before coordinates
        # and DELIMC/EOS after them, including for validation-created NOSRC.
        raw = "\n".join([
            "          -l (text) (6:2 NOSRC",
            "          ) (text) *7:4) NOPRT",
            "          . (text) (*8:6). ID=anchor NOFILL",
            "           (text) 9:1",
            "          ordinary (text) 10:2",
        ])
        self.assertEqual(tree_texts(raw), ["-l", ")", ".", "", "ordinary"])

    def test_body_and_table_attributes_are_not_text(self):
        raw = "\n".join([
            "  No (elem) *1:2",
            "  text (body) (2:3)",
            "  0L{cell} (tbl) 3:1",
        ])
        self.assertEqual(tree_texts(raw), [])


if __name__ == "__main__":
    unittest.main()
