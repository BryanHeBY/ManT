"""Compare section seams by source owner and separately by terminal row."""

from base64 import b64decode
import json
import re

from .compare import UnsupportedExtraction, device_row, tree_nodes
from .section_observations import exact, inline_value, sections


def section_node(nodes, spelling):
    heading = [node for node in nodes
               if node["kind"] == "text" and node["label"] == spelling]
    if len(heading) != 1:
        raise UnsupportedExtraction(f"no unique {spelling} source marker")
    path = heading[0]["path"]
    matches = [node for node in nodes if node["kind"] == "block"
               and node["label"] in ("SH", "SS") and node["path"] == path[:-2]]
    if len(matches) != 1:
        raise UnsupportedExtraction(f"no unique {spelling} source owner")
    return matches[0]


def between_sections(rows):
    first = [index for index, row in enumerate(rows) if row == "FIRST"]
    end = [index for index, row in enumerate(rows) if row == "ENDTEST"]
    if len(first) != 1 or len(end) != 1 or first[0] >= end[0]:
        raise UnsupportedExtraction("section interval is not uniquely delimited")
    return rows[first[0] + 1:end[0]]


def accepted_owner_rows(rows):
    """Only the fixed marker vocabulary has a known source owner."""
    if rows.count("AFTERMARK") != 1:
        raise UnsupportedExtraction("no unique printed AFTERMARK")
    after = rows.index("AFTERMARK")
    old, heading, split, structural = [], [], None, []
    for row in rows[:after]:
        if row in ("ALPHANEXT", "ALPHA NEXT", "BETA NEXT"):
            # The old word and heading may share a physical device row.
            # Structural output intentionally separates the two IR owners.
            if split is not None:
                raise UnsupportedExtraction("multiple shared heading rows")
            split = row.removesuffix("NEXT").removesuffix(" ")
            structural.extend((split, "NEXT"))
            old.append(split)
            heading.append("NEXT")
        else:
            structural.append(row)
            if row in ("ALPHA", "BETA"):
                old.append(row)
            elif row == "NEXT":
                heading.append(row)
            elif row:
                raise UnsupportedExtraction(f"unrecognized section row {row!r}")
    if len(heading) > 1 or any(rows[after + 1:]):
        raise UnsupportedExtraction("unrecognized heading/body interval")
    return dict(old=old, heading=heading, body=["AFTERMARK"], shared=split,
                structural=structural + rows[after:])


def block_text_rows(blocks):
    if any(block["type"] not in ("paragraph", "preformatted", "vertical-space")
           for block in blocks):
        raise UnsupportedExtraction("unmapped block type in fixed owner lane")
    return [row.lstrip(" ") for block in blocks
            for row in inline_value(block.get("children", ())).split("\n")
            if row.lstrip(" ")]


def check_pending_rows(source, tree, utf8, observation):
    try:
        nodes = tree_nodes(tree)
        first = section_node(nodes, "FIRST")
        target = section_node(nodes, "NEXT")
        breaks = [node["line"] for node in nodes
                  if node["kind"] == "elem" and node["label"] == "br"
                  and first["line"] < node["line"] < target["line"] and not node["noPrint"]]
        literal = any(node["kind"] == "text" and node["noFill"]
                      and first["line"] < node["line"] < target["line"] for node in nodes)
        old_word = [node for node in nodes if node["kind"] == "text" and r"\p" in node["label"]
                    and first["line"] < node["line"] < target["line"] and not node["noPrint"]]
        if len(old_word) != 1:
            raise UnsupportedExtraction("no unique preceding native word with pending break")
        authored_br = [line for line, spelling in enumerate(source.splitlines(), 1)
                       if spelling == ".br"]
        evidence = dict(first=first["line"], heading=target["line"],
                        oldWord=old_word[0]["line"], authoredBr=authored_br, nativeBr=breaks)
        if utf8["code"] != 0 or not utf8["stdout"]["utf8"]:
            raise UnsupportedExtraction("no certified native UTF-8 output")
        raw = b64decode(utf8["stdout"]["base64"], validate=True).decode()
        native = between_sections([device_row(row)[0].lstrip(" ")
                                   for row in raw.split("\n")[:-1]])
        accepted = accepted_owner_rows(native)
    except UnsupportedExtraction as error:
        return dict(state="review", reason=str(error))
    if observation is None:
        return dict(state="error", evidence=evidence, reason="missing direct observation")
    document = json.loads(observation["json"])["document"]
    by_line = {section.get("source", {}).get("line"): section
               for section in sections(document["sections"])}
    axes = {}
    axes["section-identity"] = exact("next", by_line.get(target["line"], {}).get("id"))
    if target["line"] not in by_line or first["line"] not in by_line:
        return dict(state="difference", evidence=evidence, axes=axes, native=native)
    old = by_line[first["line"]]
    new = by_line[target["line"]]
    try:
        actual = between_sections([row.lstrip(" ") for row in observation["plain"].split("\n")])
        old_rows = block_text_rows(old["blocks"])
        body_rows = block_text_rows(new["blocks"])
    except UnsupportedExtraction as error:
        return dict(state="difference", evidence=evidence, native=native, reason=str(error))
    # No strip of LF: a preceding-owner marker incorrectly published in
    # the heading is different even if the heading glyphs are unchanged.
    # In the single-word, graphless-prefix control the NBRZW *does* admit
    # one real empty pass before HEAD. Its exact row is asserted below;
    # either the heading or the preceding boundary can represent it.
    title = inline_value(new["heading"]["content"]).lstrip(" ")
    title_expected = "\n".join(accepted["heading"])
    invisible_pass = (not breaks and native == ["", "NEXT", "AFTERMARK"]
                      and any(node["kind"] == "text"
                              and first["line"] < node["line"] < target["line"]
                              and re.fullmatch(r"(?:\\p)+\\&", node["label"]) for node in nodes))
    title_axis = exact(title_expected, title)
    if invisible_pass and title == "\nNEXT":
        title_axis = ("exact",)
        evidence["acceptedInvisiblePass"] = "heading-leading-row"
    axes.update({
        "section-nesting": exact(True, new in (old["children"] if target["label"] == "SS"
                                                else document["sections"])),
        "old-content": exact(accepted["old"], old_rows),
        "heading": title_axis,
        "body-content": exact(accepted["body"], body_rows),
        "reader-rows": exact(accepted["structural"], actual),
        "transport": exact(True, all(observation[key] for key in
                                     ("validDocument", "jsonEquivalent", "ansiEquivalent"))),
    })
    for axis, blocks, line in (
        ("old-source", old["blocks"], evidence["oldWord"]),
        ("body-source", new["blocks"], target["line"] + 1),
    ):
        axes[axis] = exact(True, all(block.get("source", {}).get("line") == line
                                    for block in blocks if block_text_rows([block])))
    # The retained .br finishes a no-graph pass before the invisible
    # successor. When the old owner prints no glyph, its completed row
    # must remain in that owner, not be carried into the heading gap.
    if breaks and not literal and not accepted["old"] and native == ["", "NEXT", "AFTERMARK"]:
        axes["completed-old-row"] = exact(
            (1, 0),
            (sum(block["lines"] for block in old["blocks"] if block["type"] == "vertical-space"),
             new.get("spacingBeforeLines", 0)),
        )
    state = "difference" if any(axis[0] == "difference" for axis in axes.values()) else (
        "allowed-owner-split" if accepted["shared"] else "asserted-axes-pass")
    return dict(state=state, evidence=evidence, axes=axes, native=native, actual=actual)
