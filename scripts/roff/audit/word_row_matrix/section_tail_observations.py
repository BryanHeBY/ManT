"""Observe ordered accepted passes separately from section ownership."""

from base64 import b64decode
import json
import re

from .compare import UnsupportedExtraction, device_row, tree_nodes
from .section_facts import (
    HEAD_IDS, between_sections, inline_owners, projected_rows, section_node, suffix_blanks,
)
from .section_observations import exact, inline_value, sections

OLD = r"(?:ALPHA(?: (?:BETA|X))?|BETA|X|GAMMA)(?: <(?:https://audit\.test/|audit@example\.test)>)?"
HEAD = r"(?:NEXTX?|EXT|WORD)"
SHARED = re.compile(fr"({OLD})( *)({HEAD})")


def witness(source, tree):
    nodes = tree_nodes(tree)
    first = section_node(nodes, "FIRST")
    end = section_node(nodes, "ENDTEST")
    after = [node for node in nodes if node["label"] == "AFTERMARK" and node["kind"] == "text"]
    if len(after) != 1:
        raise UnsupportedExtraction("no unique AFTERMARK source word")
    candidates = [node for node in nodes if node["kind"] == "block"
                  and node["label"] in ("SH", "SS") and first["line"] < node["line"] < end["line"]]
    if len(candidates) > 1:
        raise UnsupportedExtraction("multiple target section owners")
    target = candidates[0] if candidates else None
    heading = None
    if target is not None:
        heading = source.splitlines()[target["line"] - 1].partition(" ")[2]
        if heading not in HEAD_IDS or after[0]["path"][:len(target["path"])] != target["path"]:
            raise UnsupportedExtraction("unmapped source heading or body owner")
    boundary = target["line"] if target is not None else after[0]["line"]
    old_nodes = [node for node in nodes
                 if first["line"] < node["line"] < boundary and not node["noPrint"]]
    old_words = [node for node in old_nodes if node["kind"] == "text"
                 and re.search(r"ALPHA|BETA|GAMMA|\\zX|\\p", node["label"])]
    if any(node["path"][:len(first["path"])] != first["path"] for node in old_words):
        raise UnsupportedExtraction("old text is not in the preceding source owner")
    requests = {name: [node["line"] for node in old_nodes
                       if node["kind"] == "elem" and node["label"] == name] for name in ("br", "sp")}
    return dict(first=first["line"], target=target["line"] if target else None,
                body=after[0]["line"], macro=target["label"] if target else None,
                heading=heading, words=sorted({node["line"] for node in old_words}),
                noFill=bool(old_words and old_words[0]["noFill"]),
                authoredBr=[line for line, text in enumerate(source.splitlines(), 1) if text == ".br"],
                nativeBr=requests["br"], nativeSp=requests["sp"])


def native_projection(native, source, evidence):
    if native.count("AFTERMARK") != 1 or any(native[native.index("AFTERMARK") + 1:]):
        raise UnsupportedExtraction("unmapped native body")
    structural, old_rows, heading_rows = [], [], []
    split = None
    for row in native[:native.index("AFTERMARK")]:
        if row == "":
            structural.append(row)
        elif re.fullmatch(fr"{OLD} *", row) and not heading_rows:
            structural.append(row)
            old_rows.append(row)
        elif re.fullmatch(HEAD, row) and evidence["target"] is not None:
            heading_rows.append(len(structural))
            structural.append(row)
        elif (match := SHARED.fullmatch(row)) and not heading_rows and evidence["target"] is not None:
            old, separator, head = match.groups()
            if len(separator) > 1:
                # No-fill can retain the author's trailing SP separately
                # from the native separator before the new heading.
                if (separator != "  " or not evidence["noFill"]
                        or not any(source.splitlines()[line - 1].endswith(" ")
                                   for line in evidence["words"])):
                    raise UnsupportedExtraction("unmapped native section separator")
                old += " "
            old_rows.append(old)
            heading_rows.append(len(structural) + 1)
            structural.extend((old, head))
            split = (old, head)
        else:
            raise UnsupportedExtraction(f"unmapped native section-tail row {row!r}")
    structural.extend(native[native.index("AFTERMARK"):])
    return structural, old_rows, heading_rows, split


def block_rows(blocks):
    return [row.lstrip(" ") for row in projected_rows(blocks)]


def visible(rows):
    positions = [index for index, row in enumerate(rows) if row]
    return rows[positions[0]:positions[-1] + 1] if positions else []


def interval(rows):
    normalized = [row.lstrip(" ") for row in rows]
    visible_rows = between_sections(normalized)
    start = normalized.index("FIRST") + 1
    return visible_rows, rows[start:start + len(visible_rows)]


def row_origin(rows, text):
    return next((len(row) - len(text) for row in rows if row.lstrip(" ") == text), None)


def accepted_heading_pad(source, evidence, accepted, native):
    # For graphless no-fill input, a retained old author SP can precede
    # the first printable heading word (term_fill/term_field after BRIND).
    # This is certified only by this literal AST seed and the +1 offset
    # measured against the following BODY's native restart origin.
    return (not accepted and evidence["noFill"] and evidence["heading"] == "NEXT"
            and len(evidence["words"]) == 1
            and re.fullmatch(r"(?:\\p\\& )+", source.splitlines()[evidence["words"][0] - 1])
            and row_origin(native, "NEXT") == row_origin(native, "AFTERMARK") + 1)


def wrapper_axes(source, expected_old, old, new):
    old_inlines = [node for block in old["blocks"] for node in block.get("children", ())]
    axes = {}
    for spelling, kind in ((r"\fB", "strong"), (".B ", "strong"),
                           (r"\fI", "emphasis"), (".I ", "emphasis")):
        if any(line.startswith(spelling) for line in source.splitlines()):
            value = "".join(inline_value(node["children"]) for node in inline_owners(old_inlines, kind))
            axes["old-font"] = exact(visible(expected_old), visible(value.split("\n")))
    for spelling, kind, key in ((".UR ", "external", "uri"), (".MT ", "email", "address")):
        targets = [line.removeprefix(spelling) for line in source.splitlines() if line.startswith(spelling)]
        if targets:
            old_label = visible(expected_old)
            old_label[-1] = old_label[-1].removesuffix(f" <{targets[0]}>")
            links = [(node["target"], inline_value(node["children"]).split("\n"))
                     for node in inline_owners(old_inlines, "link")]
            axes["old-link"] = exact([(dict(kind=kind, **{key: targets[0]}), old_label)], links)
            axes["no-heading-link"] = exact([], list(inline_owners(new["heading"]["content"], "link")))
    return axes


def check_section_tail(source, tree, utf8, observation):
    try:
        evidence = witness(source, tree)
        if utf8["code"] != 0 or not utf8["stdout"]["utf8"]:
            raise UnsupportedExtraction("no certified native UTF-8 output")
        raw = b64decode(utf8["stdout"]["base64"], validate=True).decode()
        native, native_positioned = interval([device_row(row)[0] for row in raw.split("\n")[:-1]])
        expected, accepted, heads, split = native_projection(native, source, evidence)
    except UnsupportedExtraction as error:
        return dict(state="review", reason=str(error))
    if observation is None:
        return dict(state="error", evidence=evidence, reason="missing direct observation")
    document = json.loads(observation["json"])["document"]
    by_line = {section.get("source", {}).get("line"): section
               for section in sections(document["sections"])}
    old = by_line.get(evidence["first"])
    new = by_line.get(evidence["target"]) if evidence["target"] is not None else None
    if old is None or evidence["target"] is not None and new is None:
        return dict(state="difference", evidence=evidence, reason="missing source owner", native=native)
    try:
        actual, actual_positioned = interval(observation["plain"].split("\n"))
        old_rows = block_rows(old["blocks"])
        body_rows = block_rows(new["blocks"]) if new else []
    except UnsupportedExtraction as error:
        return dict(state="difference", evidence=evidence, native=native, reason=str(error))
    # With no section seam, AFTERMARK is still part of the old owner.
    body_index = expected.index("AFTERMARK")
    old_boundary = heads[0] if heads else body_index
    expected_old = expected[:old_boundary]
    if new is None:
        expected_old = expected[:old_boundary + 1]
    elif accepted:
        # The finite domain uses PD=0 or PD=1, no negative space debt.
        # A rejection that starts in this HEAD has its own empty row.
        new_gap = int(".PD 1\n" in source) + int(
            evidence["heading"] == r"\p NEXT" and not heads and expected_old[-1:] == [""])
        if new_gap:
            expected_old = expected_old[:-new_gap]
    heading_expected = "\n".join(expected[heads[0]:body_index]) if heads else ""
    may_omit_first = (evidence["noFill"] and accepted and expected_old[:2] == ["", accepted[0]]
                      and any(block["type"] == "preformatted" for block in old["blocks"]))
    allowed = None
    if may_omit_first and actual == expected[1:]:
        expected = expected[1:]
        expected_old = expected_old[1:]
        allowed = "allowed-initial-literal-row"
    axes = {
        "reader-rows": exact(expected, actual),
        "old-content": exact([row for row in expected_old if row], [row for row in old_rows if row]),
        "old-middle-rows": exact(visible(expected_old), visible(old_rows)),
        "transport": exact(True, all(observation[key] for key in
                                     ("validDocument", "jsonEquivalent", "ansiEquivalent"))),
    }
    if accepted and new:
        axes["old-leading-rows"] = exact(
            next(i for i, row in enumerate(expected_old) if row),
            next((i for i, row in enumerate(old_rows) if row), len(old_rows)),
        )
        axes["old-tail-rows"] = exact(suffix_blanks(expected_old), suffix_blanks(old_rows))
    if new:
        title = inline_value(new["heading"]["content"]).lstrip(" ")
        # An accepted invisible old prefix with no printed old glyph can
        # appear as leading heading rows, if the full reader rows still match.
        if not accepted and heads:
            leading_rows = "\n" * heads[0]
            has_pad = accepted_heading_pad(source, evidence, accepted, native_positioned)
            if has_pad:
                origin = row_origin(actual_positioned, "NEXT")
                body_origin = row_origin(actual_positioned, "AFTERMARK")
                axes["authored-heading-pad"] = exact(
                    1, origin - body_origin if origin is not None and body_origin is not None else None)
                evidence["acceptedHeadingAuthorSpace"] = True
            if title.startswith("\n"):
                heading_expected = leading_rows + (" " if has_pad else "") + heading_expected
        axes.update({
            "heading": exact(heading_expected, title),
            "section-identity": exact(HEAD_IDS[evidence["heading"]], new["id"]),
            "section-nesting": exact(True, new in (old["children"] if evidence["macro"] == "SS"
                                                    else document["sections"])),
            "body-content": exact(["AFTERMARK"], body_rows),
            "body-source": exact(True, all(block.get("source", {}).get("line") == evidence["body"]
                                           for block in new["blocks"] if block_rows([block]))),
            "old-source": exact(True, all(block.get("source", {}).get("line") in evidence["words"]
                                          for block in old["blocks"] if any(block_rows([block])))),
            **wrapper_axes(source, expected_old, old, new),
        })
    state = ("difference" if any(axis[0] == "difference" for axis in axes.values()) else
             allowed or ("allowed-owner-split" if split else "asserted-axes-pass"))
    if split:
        evidence["sharedDeviceRow"] = split
    return dict(state=state, axes=axes, evidence=evidence, native=native, actual=actual)
