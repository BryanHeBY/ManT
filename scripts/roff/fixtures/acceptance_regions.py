"""Select structural acceptance regions without trimming executed rows.

The pristine tree supplies section ownership and footer metadata. These
helpers observe native output; they do not interpret roff words or controls.
An unqualified boundary stays uncovered instead of guessing a content range.
"""

import json
import re

from scripts.roff.lib.roff_content_compare import visible_text


SECTION = re.compile(r"^(Sh|SH) \(block\)")
META = re.compile(r'^([a-z]+)\s*=\s*(".*")$')


def native_sections(tree):
    """Return top-level section headings and whether their BODY has children."""
    sections = []
    active = None
    body_indent = None
    in_head = False
    for line in tree.splitlines():
        if SECTION.match(line):
            active = {"heading": [], "has_child": False}
            sections.append(active)
            body_indent = None
            in_head = False
        elif active is not None:
            if re.match(r"\s+(?:Sh|SH) \(head\)", line):
                in_head = True
            elif re.match(r"\s+(?:Sh|SH) \(body\)", line):
                in_head = False
                body_indent = len(line) - len(line.lstrip())
            elif in_head and " (text) " in line:
                active["heading"].append(line.strip().split(" (text) ", 1)[0])
            elif body_indent is not None and line.strip():
                if len(line) - len(line.lstrip()) > body_indent:
                    active["has_child"] = True
    for section in sections:
        section["heading"] = " ".join(section["heading"])
    return sections


def metadata(tree):
    values = {}
    for line in tree.splitlines():
        match = META.match(line)
        if match:
            try:
                values[match[1]] = json.loads(match[2])
            except json.JSONDecodeError:
                continue
    return values


def margin_projection(rows, tree, following_heading):
    """Remove only the witnessed native margin character's output column.

    term.c prints mc at the right margin, including decoration-only physical
    rows. A source character at the ordinary content origin remains intact.
    Ambiguous or changing mc controls are intentionally not qualified here.
    """
    controls = []
    lines = tree.splitlines()
    for index, line in enumerate(lines):
        if re.match(r"\s*mc \(elem\)", line):
            if index + 1 < len(lines) and " (text) " in lines[index + 1]:
                controls.append(lines[index + 1].strip().split(" (text) ", 1)[0])
            else:
                controls.append(None)
    if not controls:
        return rows, {"status": "not-applicable", "reason": "no native mc node"}
    if len(controls) != 1 or controls[0] is None or len(controls[0]) != 1:
        return rows, {"status": "uncovered", "reason": "changing or nonliteral mc"}
    character = controls[0]
    witnesses = [row for row in rows if row.startswith(following_heading)
                 and row.endswith(character)] if following_heading else []
    if len(witnesses) != 1:
        return rows, {"status": "uncovered", "reason": "no unique decorated boundary"}
    column = len(witnesses[0]) - 1
    projected = []
    removed = []
    for index, row in enumerate(rows):
        if row.endswith(character) and (len(row) - 1 == column
                                       or not row[:-1].strip(" ")):
            projected.append(row[:-1].rstrip(" "))
            removed.append(index)
        else:
            projected.append(row)
    return projected, {"status": "asserted", "character": character,
                       "column": column, "decorated_rows": removed}


def footer_start(rows, tree):
    """Authenticate the complete footer suffix, whether wrapped or on one row.

    print_man_foot()/print_mdoc_foot() print OS, date and title in that order.
    Metadata comes from this exact pristine tree, avoiding host OS guesses or
    a fixed number of Linux footer rows. Only its one preceding vspace is
    furniture; all earlier completed blank rows remain content.
    """
    meta = metadata(tree)
    if not meta.get("title") or "date" not in meta:
        return None
    title = meta["title"] + (f"({meta['sec']})" if meta.get("sec") else "")
    expected = " ".join(value for value in (meta.get("os"), meta["date"], title)
                        if value)
    for count in range(1, min(len(rows), 8) + 1):
        start = len(rows) - count
        suffix = rows[start:]
        if any(not row.strip(" ") for row in suffix):
            continue
        actual = " ".join(" ".join(row.split()) for row in suffix)
        if actual == expected and start > 0 and rows[start - 1] == "":
            return start - 1
    return None


def select_native_region(raw, tree, *, heading="DESCRIPTION", occurrence=0):
    """Select a section by AST occurrence and preserve its exact content edges."""
    sections = native_sections(tree)
    matches = [index for index, section in enumerate(sections)
               if section["heading"] == heading]
    if occurrence >= len(matches):
        return {"status": "uncovered", "reason": "missing native section owner"}
    owner = matches[occurrence]
    following = sections[owner + 1]["heading"] if owner + 1 < len(sections) else None
    rows = visible_text(raw).splitlines()
    rows, margin = margin_projection(rows, tree, following)
    if margin["status"] == "uncovered":
        return {"status": "uncovered", "reason": margin["reason"], "margin": margin}
    positions = [i for i, row in enumerate(rows) if row == heading]
    if len(positions) != len(matches):
        return {"status": "uncovered", "reason": "native heading cannot be qualified"}
    start = positions[occurrence] + 1
    if following is not None:
        ends = [i for i in range(start, len(rows)) if rows[i] == following]
        if not ends:
            return {"status": "uncovered", "reason": "missing native next section"}
        end = ends[0]
        # pre_SH()/termp_sh_pre(): a nonempty previous section requests
        # section spacing. The acceptance source headers use default PD=1.
        spacing = int(sections[owner]["has_child"])
        if spacing and (end == start or rows[end - 1] != ""):
            return {"status": "uncovered", "reason": "unqualified section spacing"}
        end -= spacing
    else:
        end = footer_start(rows, tree)
        if end is None:
            return {"status": "uncovered", "reason": "unqualified native footer"}
        spacing = 1
    return {"status": "asserted", "rows": rows[start:end],
            "owner": {"heading": heading, "occurrence": occurrence},
            "following": following, "furniture_rows": spacing, "margin": margin}


def inline_text(nodes):
    """Read section labels only; never combine both PortableDisplay projections."""
    result = []
    for node in nodes:
        kind = node.get("type")
        if kind in ("text", "code"):
            result.append(node.get("value", ""))
        elif kind == "portableDisplay":
            result.append(node.get("value", ""))
        elif "children" in node:
            result.append(inline_text(node["children"]))
    return "".join(result)


def select_product_region(raw, bundle, *, heading="DESCRIPTION", occurrence=0):
    document = bundle.get("document", {})
    sections = document.get("sections", [])
    matches = [i for i, section in enumerate(sections)
               if inline_text(section.get("heading", {}).get("content", [])) == heading]
    if occurrence >= len(matches):
        return {"status": "uncovered", "reason": "missing IR section owner"}
    owner = matches[occurrence]
    following = sections[owner + 1] if owner + 1 < len(sections) else None
    rows = visible_text(raw).splitlines()
    positions = [i for i, row in enumerate(rows) if row == heading]
    if len(positions) != len(matches):
        return {"status": "uncovered", "reason": "product heading cannot be qualified"}
    start = positions[occurrence] + 1
    if following is None:
        end, spacing = len(rows), 0
        following_heading = None
    else:
        following_heading = inline_text(following.get("heading", {}).get("content", []))
        ends = [i for i in range(start, len(rows)) if rows[i] == following_heading]
        if not ends:
            return {"status": "uncovered", "reason": "missing product next section"}
        end = ends[0]
        spacing = following.get("spacingBeforeLines", 0)
        if spacing < 0 or end - spacing < start or any(rows[end - offset - 1] != ""
                                                     for offset in range(spacing)):
            return {"status": "uncovered", "reason": "unqualified IR section spacing"}
        end -= spacing
    return {"status": "asserted", "rows": rows[start:end],
            "owner": {"heading": heading, "occurrence": occurrence},
            "following": following_heading, "furniture_rows": spacing}
