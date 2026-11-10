"""The finite audit preserves native bytes, ordered coverage and row identity."""
from base64 import b64encode
from collections import Counter
import copy
import json
import unittest

from scripts.roff.audit.word_row_matrix.compare import (
    UnsupportedExtraction, device_row, group_witness, native_prose, product_prose, word_comparison,
)
from scripts.roff.audit.word_row_matrix.core import MDOC
from scripts.roff.audit.word_row_matrix.fidelity import (
    chain_obligations, compare_direct, readback,
)
from scripts.roff.audit.word_row_matrix.manifest import (
    FOLLOWUP_MEMBERS, MEMBERS, followup_records, records,
)
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


def text(value):
    return dict(type="text", value=value)


def link(value, uri):
    return dict(type="link", target=dict(kind="external", uri=uri), children=[text(value)])


def observe(document, plain="", markdown=None):
    return dict(json=json.dumps(dict(document=document)), plain=plain, markdown=markdown)


class FirstReadbackTests(unittest.TestCase):
    def test_frozen_followup_retains_degenerate_coordinates(self):
        counts = Counter()
        unique = set()
        for cohort, _, source in followup_records():
            counts[cohort] += 1
            unique.add(source)
        self.assertEqual(counts, FOLLOWUP_MEMBERS)
        self.assertLess(len(unique), sum(counts.values()))

    def test_thirteen_mutations_fail_each_route_on_the_selected_axis(self):
        section = dict(id="body", heading=dict(content=[text("BODY")]), children=[],
                       spacingBeforeLines=1)
        paragraph = dict(type="paragraph", children=[
            text("Alpha Beta"), dict(type="line-break"),
            link("Gamma", "https://example.test/first"), text("Gamma"),
            dict(type="line-break"),
        ])
        gap = dict(type="vertical-space", lines=2)
        original = dict(sections=[dict(section, blocks=[paragraph, gap])])
        route_names = ("direct", "default", "addressable")
        for route in route_names:
            compare = (compare_direct if route == "direct" else
                       lambda a, b: readback(a, b, export=route))
            source = observe(original)
            exact = compare(source, source)
            self.assertTrue(all(exact[axis][0] == "exact" for axis in exact
                                if not axis.startswith("rendered-")), exact)
            for name, axis, operation in (
                ("first glyph", "glyph", lambda b, c: c[0].update(value="lpha Beta")),
                ("middle glyph", "glyph", lambda b, c: c[0].update(value="Alpa Beta")),
                ("last glyph", "glyph", lambda b, c: c[3].update(value="Gamm")),
                ("add author SP", "author-space", lambda b, c: c[0].update(value="Alpha  Beta")),
                ("drop author SP", "author-space", lambda b, c: c[0].update(value="AlphaBeta")),
                ("delete hard row", "hard-row", lambda b, c: c.pop(1)),
                ("hard row to gap", "hard-row",
                 lambda b, c: (c.pop(1), b.insert(1, dict(type="vertical-space", lines=1)))),
                ("duplicate owner", "owner", lambda b, c: b.insert(1, copy.deepcopy(b[0]))),
                ("drop label", "link-label", lambda b, c: c[2].update(children=[])),
                ("attach to wrong equal label", "link-owner",
                 lambda b, c: c.__setitem__(slice(2, 4), [c[3], c[2]])),
                ("drop target", "target", lambda b, c: c.__setitem__(2, c[2]["children"][0])),
                ("change positive gap", "gap", lambda b, c: b[1].update(lines=1)),
                ("drop trailing row", "trailing-row", lambda b, c: c.pop()),
            ):
                with self.subTest(route=route, mutation=name):
                    mutated = copy.deepcopy(original)
                    blocks = mutated["sections"][0]["blocks"]
                    operation(blocks, blocks[0]["children"])
                    self.assertEqual(compare(source, observe(mutated))[axis][0], "difference")

    def test_native_rewind_reviews_only_named_device_differences(self):
        literal = MDOC + ".Bd -literal\n" + r'.No "ALPHA\zX"' + "\n" + r""".No "\h'-48u' BETA" """
        expected = ["ALPHAX", "  BETA"]
        self.assertEqual(word_comparison(literal, expected, ["ALPHAX", "BETA"]),
                         "review-literal-origin")
        self.assertEqual(word_comparison(literal, expected, ["ALPHAX", " BETA"]), "difference")
        self.assertEqual(word_comparison(literal, expected, ["ALPHAX", "BET"]), "difference")
        self.assertEqual(word_comparison(literal.replace("-48u", "-47u"), expected,
                                         ["ALPHAX", "BETA"]), "review-literal-origin")
        protected = MDOC + ".Sm off\n" + r'.No "ALPHA\zX"' + "\n" + r""".No "\h'-48u'BETA" """
        self.assertEqual(word_comparison(protected, ["ALPHBETA"], ["ALPHAXBETA"]),
                         "review-protected-cell")
        self.assertEqual(word_comparison(protected, ["ALPHBETA"], ["ALPHAX"]), "difference")
        self.assertEqual(word_comparison(protected, ["ALPHBETA"], []), "difference")

    def test_a_stable_body_deletion_fails_the_first_readback(self):
        document = dict(sections=[dict(
            id="description", heading=dict(content=[text("DESCRIPTION")]),
            spacingBeforeLines=1, children=[],
            blocks=[dict(type="paragraph", children=[text("BEFOREMARK")])],
        )])
        original = observe(document, "CHECK(1)\n\nDESCRIPTION\nBEFOREMARK")
        unchanged_ir = observe(document, "CHECK\n\nDESCRIPTION\nBEFOREMAR")
        result = chain_obligations(dict(roff=original, **{
            "default-1": unchanged_ir, "default-2": unchanged_ir,
        }))
        self.assertEqual(result["default-1"]["from-source"]["rendered-content-row"][0],
                         "difference")
        self.assertEqual(result["default-2"]["from-first"]["rendered-content-row"][0], "exact")

    def test_exact_layout_and_markdown_framing_controls(self):
        def document(blocks):
            return dict(sections=[dict(id="body", heading=dict(content=[text("BODY")]),
                                       spacingBeforeLines=1, blocks=blocks, children=[])])
        source = document([dict(type="preformatted", children=[text(" A\nB\n")],
                                layout=dict(spacingBeforeLines=1),
                                inlineLayout=dict(rowHints=[dict(row=1, indentColumns=-1, firstLineIndentColumns=4)]))])
        read = document([dict(type="paragraph", children=[text("\n")]),
                         dict(type="preformatted", children=[text(" A\n   B\n")])])
        self.assertEqual(readback(observe(source), observe(read), export="default")["gap"][0],
                         "exact")
        self.assertEqual(readback(observe(source), observe(read), export="default")["author-space"][0],
                         "exact")
        self.assertEqual(readback(observe(source), observe(read), export="default")["trailing-row"][0],
                         "exact")
        read["sections"][0]["blocks"][1]["children"][0]["value"] = "A\n   B\n"
        self.assertEqual(readback(observe(source), observe(read), export="default")["author-space"][0],
                         "difference")

    def test_native_query_label_and_interior_spacing_have_fixed_projections(self):
        first = dict(type="paragraph", children=[text("A")])
        last = dict(type="paragraph", children=[text("B")])
        source = dict(meta=dict(title="CHECK"), sections=[], blocks=[
            first, dict(type="vertical-space", lines=1), last,
        ])
        observed = dict(sections=[], heading=dict(content=[text("check")]), blocks=[
            first, dict(last, layout=dict(spacingBeforeLines=1)),
        ])
        wire_source = dict(json=json.dumps(dict(document=source, label="check")), plain="")
        status = readback(wire_source, observe(observed), export="default")
        self.assertEqual(status["owner"][0], "exact")
        self.assertEqual(status["gap"][0], "exact")
        self.assertEqual(status["glyph"][0], "exact")
        observed["heading"]["content"] = [text("chec")]
        observed["blocks"][1] = last
        status = readback(wire_source, observe(observed), export="default")
        self.assertEqual(status["glyph"][0], "difference")
        self.assertEqual(status["gap"][0], "difference")

    def test_interior_gap_cannot_be_reassigned_to_navigation_only(self):
        first = dict(type="paragraph", children=[text("A")])
        marker = dict(type="paragraph", children=[link("", "https://example.test")])
        source = dict(sections=[], blocks=[first, dict(type="vertical-space", lines=1), marker])
        wrong = dict(sections=[], blocks=[first, dict(marker, layout=dict(spacingBeforeLines=1))])
        for export in ("default", "addressable"):
            self.assertEqual(readback(observe(source), observe(wrong), export=export)["gap"][0],
                             "difference")

    def test_raw_anchor_exception_requires_whole_source_span_and_destination(self):
        marker = '<a id="body"></a>'
        authored = '\\' + marker
        original = dict(sections=[dict(id="body", heading=dict(content=[text("BODY")]),
                                       spacingBeforeLines=1, blocks=[], children=[])])
        read = dict(original, blocks=[dict(type="paragraph", children=[text(marker)],
                                          source=dict(byteRange=dict(start=0, end=len(marker))))])
        self.assertEqual(readback(observe(original), observe(read, markdown=marker + "\n## BODY"),
                                  export="addressable")["owner"][0], "exact")
        self.assertEqual(readback(observe(original), observe(read, markdown=authored + "\n## BODY"),
                                  export="addressable")["owner"][0], "difference")
        read["blocks"][0]["layout"] = dict(spacingBeforeLines=1)
        self.assertEqual(readback(observe(original), observe(read, markdown=marker + "\n## BODY"),
                                  export="addressable")["gap"][0], "difference")


    def test_addressable_proof_follows_section_preorder(self):
        def section(name, children=()):
            return dict(id=name.lower(), heading=dict(content=[text(name)]),
                        spacingBeforeLines=1, blocks=[], children=list(children))
        source = dict(sections=[section("PARENT", [section("CHILD")]), section("NEXT")])
        md = '## PARENT\n<a id="child"></a>\n### CHILD\n<a id="next"></a>\n## NEXT'
        observed = copy.deepcopy(source)
        parent, child = observed["sections"][0], observed["sections"][0]["children"][0]
        for node, target in ((parent, "child"), (child, "next")):
            marker = f'<a id="{target}"></a>'
            start = md.index(marker)
            node["blocks"] = [dict(type="paragraph", children=[text(marker)],
                                   source=dict(byteRange=dict(start=start, end=start + len(marker))))]
        result = readback(observe(source), observe(observed, markdown=md), export="addressable")
        self.assertEqual(result["owner"][0], "exact")
        self.assertEqual(result["glyph"][0], "exact")
        child["blocks"][0]["source"]["byteRange"]["start"] += 1
        result = readback(observe(source), observe(observed, markdown=md), export="addressable")
        self.assertEqual(result["glyph"][0], "difference")


if __name__ == "__main__":
    unittest.main()
