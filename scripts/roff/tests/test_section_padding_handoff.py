"""The section-padding lane must not admit duplicate or shifted rows."""

from collections import Counter
import json
from unittest import TestCase

from scripts.roff.audit.word_row_matrix.lanes import SECTION_PADDING_MEMBERS
from scripts.roff.audit.word_row_matrix.oracle import stream_bytes
from scripts.roff.audit.word_row_matrix.section_facts import projected_rows
from scripts.roff.audit.word_row_matrix.section_padding import operand, padding_sources
from scripts.roff.audit.word_row_matrix.section_padding_observations import check_section_padding
from scripts.roff.audit.word_row_matrix.section_tail import render
from scripts.roff.tests.test_section_tail_handoff import text_block


def observe(word, native, old, rows, *, title="NEXT", wrapper=None, body_children=None):
    # Pinned CVS tree/UTF-8 slices. Positive HORIZ is a buffered NBRSP,
    # not a printed graph; the old reading retains its one SP. The
    # following SS's term_newln consumes both intervals (term.c:678-700,
    # man_term.c::post_SH). The UR control has a nested source BODY.
    operand_tree = f"      {word} (text) *7:1 NOFILL\n"
    heading_line = 8
    source_body = word + "\n"
    if wrapper == "UR":
        operand_tree = f"""      UR (block) *7:2 NOFILL
        UR (head) 7:2 NOFILL
            https://audit.test/ (text) 7:5 NOFILL
        UR (body) 7:2 NOFILL
            {word} (text) *8:1 NOFILL
"""
        heading_line = 10
        source_body = f".UR https://audit.test/\n{word}\n.UE\n"
    elif wrapper == "I":
        operand_tree = f"""      I (elem) *7:2 NOFILL
          {word} (text) 7:4 NOFILL
"""
        source_body = f".I {word}\n"
    head = heading_line
    tree = f"""SH (block) *4:2
  SH (head) 4:2 ID=HREF
      FIRST (text) 4:5
  SH (body) 4:2
{operand_tree}      SS (block) *{head}:2
        SS (head) {head}:2 ID=HREF
            NEXT (text) {head}:5
        SS (body) {head}:2
            AFTERMARK (text) *{head + 1}:1
SH (block) *{head + 2}:2
  SH (head) {head + 2}:2 ID=HREF
      ENDTEST (text) {head + 2}:5
""".encode()
    body = text_block("AFTERMARK", head + 1)
    if body_children is not None:
        body["children"] = body_children
    next_section = dict(id="next", source=dict(line=head), children=[],
                        heading=dict(content=text_block(title)["children"]),
                        blocks=[body])
    first = dict(id="first", source=dict(line=4), blocks=old, children=[next_section])
    observation = dict(json=json.dumps(dict(document=dict(sections=[first]))),
                       plain="\n".join(["FIRST", *rows, "ENDTEST"]),
                       validDocument=True, jsonEquivalent=True, ansiEquivalent=True)
    utf8 = dict(code=0, stdout=stream_bytes("\n".join(["FIRST", *native, "ENDTEST", ""]).encode()))
    return check_section_padding(render(source_body, literal=True), tree, utf8, observation)


def literal(value, line=7):
    return text_block(value, line, "preformatted")


class SectionPaddingHandoffTests(TestCase):
    def test_frozen_core_and_ordered_axes(self):
        records = list(padding_sources())
        self.assertEqual(Counter(cohort for cohort, _, _ in records), SECTION_PADDING_MEMBERS)
        self.assertEqual(len({source for _, _, source in records}), 2111)
        core = {tuple(axes): source for cohort, axes, source in records if cohort == "PHD-core"}
        self.assertIn("\nALPHA \n", core[True, 0, "ascii-space", "", "SS", 0])
        self.assertIn("\nALPHA\n", core[True, 0, "none", "", "SS", 0])
        self.assertIn("\nALPHA\\p \\p\\&\t\n", core[True, 1, "tab", "", "SS", 0])
        self.assertIn("\nALPHA\\p \\p\\& \u00a0\n", core[True, 1, "nbsp", "", "SS", 0])
        self.assertTrue(any(cohort == "PHD-eof" and ".SH ENDTEST" not in source
                            for cohort, _, source in records))

    def test_one_retained_motion_row_does_not_allow_replayed_completed_rows(self):
        word = operand(1, r"\h'1n'")
        native = ["ALPHA", "", "NEXT", "AFTERMARK"]
        rows = ["ALPHA", "", "", "NEXT", "AFTERMARK"]
        expected = literal("ALPHA\n\n ")
        self.assertEqual(observe(word, native, [expected], rows)["state"],
                         "allowed-reading-space-row")
        for extra in (1, 2):
            duplicate = observe(word, native, [expected, dict(type="vertical-space", lines=extra)],
                                rows)
            self.assertEqual(duplicate["axes"]["old-tail-rows"], ("difference", 2, 2 + extra))
            self.assertEqual(duplicate["state"], "difference")
        erased = observe(word, native, [literal("ALPHA\n\n")], rows)
        self.assertEqual(erased["axes"]["old-reading-padding"], ("difference", " ", ""))
        moved = observe(word, native, [literal("ALPHA\n\n")], rows, title=" NEXT")
        self.assertEqual(moved["axes"]["old-reading-padding"], ("difference", " ", ""))
        missing_break = observe(word, native, [literal("ALPHA\n ")], rows)
        self.assertEqual(missing_break["axes"]["old-literal-interior"],
                         ("difference", ["ALPHA", ""], ["ALPHA", " "]))

    def test_authored_nbsp_is_not_reading_padding_or_an_empty_scalar(self):
        word = operand(1, "\u00a0")
        native = ["ALPHA", "", "\u00a0 NEXT", "AFTERMARK"]
        rows = ["ALPHA", "", "\u00a0", "NEXT", "AFTERMARK"]
        good = observe(word, native, [literal("ALPHA\n\n\u00a0")], rows)
        self.assertEqual(good["state"], "allowed-owner-split")
        self.assertEqual(good["axes"]["author-nbsp"], ("exact",))
        for replacement in ("", " "):
            changed = observe(word, native, [literal("ALPHA\n\n" + replacement)], rows)
            self.assertEqual(changed["axes"]["author-nbsp"], ("difference", 1, 0))
            self.assertEqual(changed["state"], "difference")

    def test_rejected_old_payload_is_not_allowed_by_whitespace_normalization(self):
        word = operand(1, r"\h'1n'", r"\p BETA")
        native = ["ALPHA", "", "", "", "AFTERMARK"]
        old = [literal("ALPHA"), dict(type="vertical-space", lines=3)]
        self.assertEqual(observe(word, native, old, native, title="")["state"], "asserted-axes-pass")
        revived = observe(word, native, [literal("ALPHA\n\nBETA")], native, title="")
        self.assertEqual(revived["axes"]["old-content"], ("difference", ["ALPHA"], ["ALPHA", "BETA"]))

    def test_every_payload_segment_keeps_its_original_source(self):
        # The pinned tree for this input puts ALPHA and BETA in the same
        # no-fill source node on line 7 (term.c::term_word); BETA can share
        # NEXT's terminal row without moving into NEXT's source.
        word = operand(1, r"\h'1n' BETA")
        native = ["ALPHA", "", "BETA NEXT", "AFTERMARK"]
        rows = ["ALPHA", "", "BETA", "NEXT", "AFTERMARK"]
        original = [literal("ALPHA\n"), literal("  BETA")]
        self.assertEqual(observe(word, native, original, rows)["state"], "allowed-owner-split")
        no_source = literal("  BETA")
        del no_source["source"]
        for moved in (literal("  BETA", 8), no_source):
            result = observe(word, native, [original[0], moved], rows)
            self.assertEqual(result["axes"]["old-content"], ("exact",))
            self.assertEqual(result["axes"]["old-source"], (
                "difference", [7, 7], [7, moved.get("source", {}).get("line")]))
            self.assertEqual(result["state"], "difference")

    def test_raw_rows_keep_a_wrapped_empty_literal_but_not_an_empty_paragraph(self):
        empty = [dict(type="emphasis", children=[dict(type="text", value="")])]
        self.assertEqual(projected_rows([dict(type="preformatted", children=empty)]), [""])
        self.assertEqual(projected_rows([dict(type="paragraph", children=empty)]), [])
        self.assertEqual(projected_rows([dict(
            type="paragraph", children=[dict(type="line-break")])]), [""])

    def test_equal_reading_text_does_not_hide_missing_old_style_or_target(self):
        word = operand(1, r"\h'1n' BETA")
        native = ["ALPHA", "", "BETA NEXT", "AFTERMARK"]
        rows = ["ALPHA", "", "BETA", "NEXT", "AFTERMARK"]
        children = literal("ALPHA\n\n  BETA")["children"]
        emphasized = dict(literal(""), children=[dict(type="emphasis", children=children)])
        styled = observe(word, native, [emphasized], rows, wrapper="I")
        self.assertEqual(styled["axes"]["old-font"], ("exact",))
        lost_style = observe(word, native, [literal("ALPHA\n\n  BETA")], rows, wrapper="I")
        self.assertEqual(lost_style["axes"]["old-font"], ("difference", "ALPHA\n\n  BETA", ""))

        suffix = " <https://audit.test/>"
        native = ["ALPHA", "", f"BETA{suffix} NEXT", "AFTERMARK"]
        rows = ["ALPHA", "", f"BETA{suffix}", "NEXT", "AFTERMARK"]
        target = dict(kind="external", uri="https://audit.test/")
        linked = dict(literal("", 8), children=[
            dict(type="link", children=children, target=target), dict(type="text", value=suffix)])
        actual = observe(word, native, [linked], rows, wrapper="UR")
        self.assertEqual(actual["axes"]["old-link-target"], ("exact",))
        lost_link = observe(word, native, [literal("ALPHA\n\n  BETA" + suffix, 8)],
                            rows, wrapper="UR")
        self.assertEqual(lost_link["axes"]["old-link-target"], ("difference", [target], []))
        short_label = dict(literal("", 8), children=[
            dict(type="link", children=children[:1], target=target),
            *children[1:], dict(type="text", value=suffix)])
        shortened = observe(word, native, [short_label], rows, wrapper="UR")
        self.assertEqual(shortened["axes"]["old-link-label"],
                         ("difference", ["ALPHA\n\n  BETA"], ["ALPHA"]))

    def test_body_style_alone_does_not_change_the_content_axis(self):
        word = operand(1, r"\h'1n'")
        native = ["ALPHA", "", "NEXT", "AFTERMARK"]
        rows = ["ALPHA", "", "", "NEXT", "AFTERMARK"]
        styled = [dict(type="strong", children=[dict(type="text", value="AFTERMARK")])]
        result = observe(word, native, [literal("ALPHA\n\n ")], rows, body_children=styled)
        self.assertEqual(result["state"], "allowed-reading-space-row")
        self.assertEqual(result["axes"]["body"], ("exact",))
