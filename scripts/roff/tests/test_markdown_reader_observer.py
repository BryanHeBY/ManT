"""Consumer mutations for the pristine-bound RC05 core JSON observer.

The exact 100 roff sources ran the verified five-profile reference before
these assertions were written (the permanent cases.json carries the hashes).
These tests mutate IR facts, not native output or the source grammar. Native
ESCAPE_BREAK cells follow term.c::term_word/term_fill; container projection
follows the documented portable Markdown contract and remains a separate axis.
"""

import copy
import hashlib
import unittest

from scripts.roff.fixtures.markdown_reader_observer import markdown_reader_axes
from scripts.roff.fixtures.markdown_rule_cases import cases


def case(container="paragraph", hardline="interior-empty", carrier="No"):
    return next(one for one in cases() if one["metadata"]["container"] == container
                and one["metadata"]["hardline"] == hardline
                and one["metadata"]["carrier"] == carrier)


def text(value):
    return {"type": "text", "value": value}


def phrasing(children, kind="paragraph"):
    return {"type": kind, "children": children}


def bundle(block):
    return {"document": {"sections": [{"heading": {"content": [text("DESCRIPTION")]},
                                       "blocks": [block]}]}}


def definition(children, relation="separate"):
    return bundle({"type": "definition-list", "items": [{"source": {"line": 9},
        "terms": [children], "description": [phrasing([text("BodyWord")])],
        "layout": {"headBodyRelation": relation}}]})


def bullet(children):
    return bundle({"type": "list", "kind": {"kind": "bullet"},
                   "items": [{"blocks": [phrasing(children)]}]})


def good(report):
    return all(value is True for value in report.values())


class MarkdownReaderObserverTests(unittest.TestCase):
    def test_edges_repeated_hard_rows_and_internal_separators_are_observable(self):
        one = case(hardline="leading-two")
        expected = "\n\nA\n\nAFTER\n\n"
        original = bundle(phrasing([text(expected)]))
        self.assertTrue(good(markdown_reader_axes(one, original, original)))
        for index, character in enumerate(expected):
            if character != "\n":
                continue
            wrong = bundle(phrasing([text(expected[:index] + expected[index + 1:])]))
            with self.subTest(index=index):
                self.assertFalse(markdown_reader_axes(one, original, wrong)["rows"])
        original = bundle(phrasing([text("A AFTER")]))
        self.assertFalse(markdown_reader_axes(one, original,
            bundle(phrasing([text("AAFTER")])))["separators"])

    def test_visible_unicode_controls_and_internal_nbsp_are_not_normalized(self):
        one = case()
        expected = "A中e\u0301\u00a0AFTER"
        original = bundle(phrasing([text(expected)]))
        for wrong in ("A文e\u0301\u00a0AFTER", "A中e\u00a0AFTER",
                      "A中e\u0301 AFTER", expected + "\x1b", expected + "�"):
            with self.subTest(wrong=wrong):
                self.assertFalse(markdown_reader_axes(one, original,
                    bundle(phrasing([text(wrong)])))["content"])

    def test_hidden_link_device_suffix_is_projected_with_its_hard_rows(self):
        one = case(carrier="Lk")
        original = bundle(phrasing([text("A"), {"type": "portable-display",
            "display": "", "children": [text(": https://ex.org"),
                                            {"type": "line-break"}, text(" ")]}]))
        reader = bundle(phrasing([text("A\n")]))
        self.assertTrue(good(markdown_reader_axes(one, original, reader)))
        self.assertFalse(markdown_reader_axes(one, original,
            bundle(phrasing([text("A")])))["rows"])

    def test_definition_bullet_does_not_create_a_content_glyph_or_duplicate_origin(self):
        one = case(container="tag")
        original = definition([text("A"), {"type": "line-break", "indentColumns": 10},
                               text("AFTER")])
        reader = bullet([text("A\n" + "\u00a0" * 10 + "AFTER\nBodyWord")])
        self.assertTrue(good(markdown_reader_axes(one, original, reader)))
        for wrong in ("A\nAFTER\n", "A\nAFTERBodyWord", "A\n\nAFTER\nBodyWord"):
            with self.subTest(wrong=wrong):
                self.assertFalse(good(markdown_reader_axes(one, original, bullet([text(wrong)]))))

    def test_body_ownership_and_original_source_are_distinct_from_equal_text(self):
        one = case(container="hang")
        original = definition([text("A")], relation="run-in")
        reader = bullet([text("A BodyWord")])
        self.assertTrue(good(markdown_reader_axes(one, original, reader)))
        wrong = copy.deepcopy(original)
        wrong["document"]["sections"][0]["blocks"][0]["items"][0]["source"]["line"] += 1
        self.assertFalse(markdown_reader_axes(one, wrong, reader)["body-ownership"])
        wrong = definition([text("A BodyWord")], relation="run-in")
        wrong["document"]["sections"][0]["blocks"][0]["items"][0]["description"] = [phrasing([])]
        self.assertFalse(markdown_reader_axes(one, wrong, reader)["body-ownership"])

    def test_column_fence_keeps_all_left_rows_and_the_real_neighbor(self):
        one = case(container="column", hardline="leading-one")
        original = bundle({"type": "table", "rows": [{"cells": [
            {"blocks": [phrasing([text("\nAFTER")])]},
            {"blocks": [phrasing([text("RIGHT")])]}]}]})
        reader = bundle(phrasing([text("\nAFTER | RIGHT")], "preformatted"))
        self.assertTrue(good(markdown_reader_axes(one, original, reader)))
        for wrong in ("AFTER | RIGHT", "\nAFTER | WRONG", "\nAFTER | RIGHTEXTRA"):
            with self.subTest(wrong=wrong):
                self.assertFalse(good(markdown_reader_axes(one, original,
                    bundle(phrasing([text(wrong)], "preformatted")))))

    def test_plain_fence_retains_terminal_suffix_without_claiming_active_identity(self):
        one = case(container="literal", carrier="Lk")
        original = bundle(phrasing([text("A\nAFTER"), {"type": "portable-display",
            "display": "", "children": [text(": https://ex.org")]}], "preformatted"))
        reader = bundle(phrasing([text("A\nAFTER: https://ex.org")], "preformatted"))
        report = markdown_reader_axes(one, original, reader)
        self.assertTrue(good(report))
        self.assertNotIn("link-identity", report)

    def test_source_and_metadata_mutations_do_not_expand_qualification(self):
        one = case()
        original = bundle(phrasing([text("A\n\nAFTER")]))
        changed = dict(one, source=one["source"] + ".No AUTHOR\n")
        self.assertFalse(markdown_reader_axes(changed, original, original)["source-binding"])
        changed = copy.deepcopy(one)
        changed["metadata"]["container"] = "literal"
        self.assertFalse(markdown_reader_axes(changed, original, original)["source-binding"])
        alias = dict(one, id="retained-source-alias", metadata={"context": "paragraph", "carrier": "No"})
        self.assertTrue(good(markdown_reader_axes(alias, original, original)))
        outside = dict(source="ordinary Markdown", source_sha256=
            hashlib.sha256(b"ordinary Markdown").hexdigest(), metadata={})
        self.assertEqual(set(markdown_reader_axes(outside, original, original).values()), {"uncovered"})

    def test_unsupported_html_or_wrong_container_cannot_impersonate_phrasing(self):
        one = case(hardline="leading-one")
        original = bundle(phrasing([text("\nAFTER")]))
        for wrong in ({"type": "unsupported", "value": "<br>\nAFTER"},
                      phrasing([text("\nAFTER")], "preformatted")):
            with self.subTest(wrong=wrong):
                self.assertFalse(good(markdown_reader_axes(one, original, bundle(wrong))))
        wrong = bundle(phrasing([text("\nAFTER")]))
        wrong["document"]["sections"][0]["blocks"].append(phrasing([text("INJECTED")]))
        self.assertFalse(good(markdown_reader_axes(one, original, wrong)))


if __name__ == "__main__":
    unittest.main()
