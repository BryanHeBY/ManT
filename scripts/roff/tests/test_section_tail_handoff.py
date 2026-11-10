"""Mutation tests for the ordered section-tail oracle and owner axes."""

from collections import Counter
import json
from unittest import TestCase

from scripts.roff.audit.word_row_matrix.manifest import SECTION_TAIL_MEMBERS
from scripts.roff.audit.word_row_matrix.oracle import stream_bytes
from scripts.roff.audit.word_row_matrix.section_tail import render, section_tail_sources, word
from scripts.roff.audit.word_row_matrix.section_tail_observations import check_section_tail


def text_block(value, line=6, kind="paragraph"):
    children = []
    for index, row in enumerate(value.split("\n")):
        if index:
            children.append(dict(type="line-break"))
        if row:
            children.append(dict(type="text", value=row))
    return dict(type=kind, children=children, source=dict(line=line))


def observe(word_text, native, old, rows, *, literal=False, heading="NEXT"):
    # Minimal slices of the pinned CVS tree and native UTF-8 fixtures.
    # term_fill can accept the NBRZW row without printing a glyph.
    word_line = 7 if literal else 6
    head = word_line + 1
    tree = f"""SH (block) *4:2
  SH (head) 4:2 ID=HREF
      FIRST (text) 4:5
  SH (body) 4:2
      {word_text} (text) *{word_line}:1{' NOFILL' if literal else ''}
      SS (block) *{head}:2
        SS (head) {head}:2 ID=HREF
            NEXT (text) {head}:5
        SS (body) {head}:2
            AFTERMARK (text) *{head + 1}:1
SH (block) *{head + 2}:2
  SH (head) {head + 2}:2 ID=HREF
      ENDTEST (text) {head + 2}:5
""".encode()
    next_section = dict(id="next", source=dict(line=head), children=[],
                        heading=dict(content=text_block(heading)["children"]),
                        blocks=[text_block("AFTERMARK", head + 1)])
    first = dict(id="first", source=dict(line=4), blocks=old, children=[next_section])
    observation = dict(json=json.dumps(dict(document=dict(sections=[first]))),
                       plain="\n".join(["FIRST", *rows, "ENDTEST"]),
                       validDocument=True, jsonEquivalent=True, ansiEquivalent=True)
    utf8 = dict(code=0, stdout=stream_bytes("\n".join(["FIRST", *native, "ENDTEST", ""]).encode()))
    return check_section_tail(render(word_text + "\n", literal=literal), tree, utf8, observation)


class SectionTailHandoffTests(TestCase):
    def test_frozen_matrix_includes_ordered_old_passes_and_independent_controls(self):
        records = list(section_tail_sources())
        self.assertEqual(Counter(cohort for cohort, _, _ in records), SECTION_TAIL_MEMBERS)
        self.assertEqual(len({source for _, _, source in records}), 600)
        for prefix, tail in (("ALPHA", "BETA"), ("ALPHA", r"\zX"),
                             ("ALPHA", r"\p BETA"), ("ALPHA", "")):
            expected = word(2, prefix, tail)
            self.assertIn((expected + "\n"),
                          next(source for cohort, axes, source in records if cohort == "PHT-core"
                               and axes == [2, prefix, tail, "", False, "SS", 0]))

    def test_same_device_row_does_not_allow_replaying_or_moving_old_blank_rows(self):
        word_text = r"ALPHA\p \p\& BETA"
        native = ["ALPHA", "", "BETA NEXT", "AFTERMARK"]
        expected = ["ALPHA", "", "BETA", "NEXT", "AFTERMARK"]
        old = text_block("ALPHA\n\nBETA")
        self.assertEqual(observe(word_text, native, [old], expected)["state"], "allowed-owner-split")
        duplicated = observe(word_text, native, [old, dict(type="vertical-space", lines=1)], expected)
        self.assertEqual(duplicated["axes"]["old-tail-rows"], ("difference", 0, 1))
        moved = observe(word_text, native, [text_block("ALPHA\nBETA\n\n")], expected)
        self.assertEqual(moved["axes"]["old-middle-rows"],
                         ("difference", ["ALPHA", "", "BETA"], ["ALPHA", "BETA"]))
        self.assertEqual(moved["axes"]["old-tail-rows"], ("difference", 0, 1))
        leaked = observe(word_text, native, [text_block("ALPHA")], expected, heading="BETA NEXT")
        self.assertEqual(leaked["axes"]["old-content"], ("difference", ["ALPHA", "BETA"], ["ALPHA"]))
        self.assertEqual(leaked["axes"]["heading"], ("difference", "NEXT", "BETA NEXT"))

    def test_new_heading_cannot_consume_real_old_tail_and_rejection_cannot_revive_old_word(self):
        native = ["ALPHA", "", "", "NEXT", "AFTERMARK"]
        old = [text_block("ALPHA"), dict(type="vertical-space", lines=2)]
        word_text = r"ALPHA\p \p\& \p\&"
        self.assertEqual(observe(word_text, native, old, native)["state"], "asserted-axes-pass")
        erased = observe(word_text, native, old[:1], native)
        self.assertEqual(erased["axes"]["old-tail-rows"], ("difference", 2, 0))
        rejected = r"ALPHA\p \p\& \p BETA"
        rows = ["ALPHA", "", "", "AFTERMARK"]
        self.assertEqual(observe(rejected, rows, old, rows, heading="")["state"], "asserted-axes-pass")
        revived = observe(rejected, rows, [text_block("ALPHA\n\nBETA")], rows, heading="")
        self.assertEqual(revived["axes"]["old-content"], ("difference", ["ALPHA"], ["ALPHA", "BETA"]))

    def test_old_delayed_glyph_stays_with_the_accepted_old_tail(self):
        word_text = r"ALPHA\p \p\& \zX"
        native = ["ALPHA", "", "XNEXT", "AFTERMARK"]
        rows = ["ALPHA", "", "X", "NEXT", "AFTERMARK"]
        result = observe(word_text, native, [text_block("ALPHA\n\nX")], rows)
        self.assertEqual(result["state"], "allowed-owner-split")
        self.assertEqual(result["evidence"]["sharedDeviceRow"], ("X", "NEXT"))
        moved = observe(word_text, native, [text_block("ALPHA")], rows, heading="XNEXT")
        self.assertEqual(moved["axes"]["old-content"], ("difference", ["ALPHA", "X"], ["ALPHA"]))

    def test_only_the_single_initial_literal_row_can_be_omitted(self):
        old = [text_block("BETA", 7, "preformatted")]
        rows = ["BETA", "NEXT", "AFTERMARK"]
        native = ["", "BETA NEXT", "AFTERMARK"]
        single = observe(r"\p\& BETA", native, old, rows, literal=True)
        self.assertEqual(single["state"], "allowed-initial-literal-row")
        twice = observe(r"\p\& \p\& BETA", ["", *native], old, rows, literal=True)
        self.assertEqual(twice["axes"]["old-leading-rows"], ("difference", 2, 0))
        self.assertEqual(twice["state"], "difference")

    def test_graphless_native_heading_origin_keeps_only_the_retained_author_space(self):
        # Paired pristine probes with and without the authored trailing
        # blank begin NEXT at columns 6 and 5 respectively.
        native = ["", "      NEXT", "     AFTERMARK"]
        actual = ["", "   NEXT", "  AFTERMARK"]
        seed = "\\p\\& "
        kept = observe(seed, native, [], actual, literal=True, heading="\n NEXT")
        self.assertEqual(kept["state"], "asserted-axes-pass")
        self.assertEqual(kept["axes"]["authored-heading-pad"], ("exact",))
        lost = observe(seed, native, [], ["", "  NEXT", "  AFTERMARK"],
                       literal=True, heading="\nNEXT")
        self.assertEqual(lost["axes"]["authored-heading-pad"], ("difference", 1, 0))
        extra = observe(seed.rstrip(), ["", "     NEXT", "     AFTERMARK"], [], actual,
                        literal=True, heading="\n NEXT")
        self.assertEqual(extra["axes"]["heading"], ("difference", "\nNEXT", "\n NEXT"))
