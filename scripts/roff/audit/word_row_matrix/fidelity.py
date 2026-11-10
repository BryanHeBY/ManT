"""Independent source-to-readback checks for the bounded Markdown lane.

Rows and owner paths are compared without trimming. The unsupported shapes
are explicit obligations, not passes inferred from two equal readbacks.
"""
import json
import re

AXES = ("glyph", "author-space", "hard-row", "owner", "link-label",
        "link-owner", "target", "gap", "trailing-row", "rendered-row", "rendered-content-row")


class Review(ValueError):
    """The projection has no selected policy for this source shape."""


def inline_signature(nodes, *, retained_links=None):
    parts, links = [], []
    offset = 0

    def visit(children):
        nonlocal offset
        for node in children:
            kind = node["type"]
            if kind in ("text", "code", "line-break"):
                value = "\n" if kind == "line-break" else node["value"]
                parts.append(value)
                offset += len(value)
            elif kind in ("strong", "emphasis"):
                visit(node["children"])
            elif kind == "link":
                start = offset
                visit(node["children"])
                target = node["target"]
                if retained_links is None or target["kind"] in retained_links:
                    links.append((start, offset, target))
            else:
                raise Review(f"inline {kind!r} needs its own projection rule")
    visit(nodes)
    return "".join(parts), links


def layout_rows(value, hints):
    """Materialize only the origin that the fenced-code export carries."""
    if "\r" in value:
        raise Review("literal CR normalization needs a source-row mapping")
    rows = value.split("\n")
    for hint in hints:
        index = hint["row"]
        if index >= len(rows):
            raise Review("literal origin outside the source row count")
        indent = min(4096, max(0, hint.get("indentColumns", 0)
                               + hint.get("firstLineIndentColumns", 0)))
        if rows[index]:
            rows[index] = " " * indent + rows[index]
    return "\n".join(rows)


def anchor_lines(target):
    if target is None:
        return ()
    identity = target["id"]
    ids = (identity, *(alias for alias in target.get("fragmentAliases", []) if alias != identity))
    # Only the fixed alphabet emitted verbatim by the bounded source lane.
    if any(not re.fullmatch(r"[a-zA-Z0-9_.:-]+", value) for value in ids):
        return ()
    return tuple(f'<a id="{value}"></a>' for value in ids)


def anchor_block(block, markdown, lines, *, predecessor=None, inherited=False):
    """Prove a standalone marker's spelling, span and source-owned identity."""
    if not lines or markdown is None or block["type"] != "paragraph":
        return False
    span = block.get("source", {}).get("byteRange")
    if not span or block["children"] != [{"type": "text", "value": " ".join(lines)}]:
        return False
    if block.get("inlineLayout"):
        return False
    gap = 0
    if predecessor is not None:
        previous = predecessor.get("source", {}).get("byteRange")
        if not previous or previous["end"] > span["start"]:
            return False
        # This examines only the syntax boundary, never the owner's text
        # contribution. A known framing blank cannot excuse a changed gap.
        boundary = markdown[previous["start"]:previous["end"]].rstrip(b" \t\r\n")
        start = previous["start"] + len(boundary)
        gap = int(re.search(rb"\n[ \t]*\n", markdown[start:span["start"]]) is not None)
    expected_layout = {"spacingBeforeLines": gap} if gap else {}
    if block.get("layout", {}) != expected_layout:
        return False
    spelling = (" ".join(lines).replace("<", r"\<").replace(">", r"\>")
                if inherited else "\n".join(lines))
    raw = markdown[span["start"]:span["end"]]
    return raw in (spelling.encode(), (spelling + "\n").encode())


def section_anchors(document, markdown, source, inherited=None):
    """Return proven block exclusions and their per-boundary source spellings."""
    exclusions, carried = set(), {}
    if markdown is None:
        return exclusions, carried
    markdown = markdown.encode()
    inherited = inherited or {}

    def trailing(path, blocks, following):
        index = len(blocks) - 1
        lines = anchor_lines(following)
        if index >= 0 and anchor_block(blocks[index], markdown, lines,
                                       predecessor=blocks[index - 1] if index else None):
            exclusions.add((path, index))
            carried[path] = lines
            index -= 1
        if index >= 0 and anchor_block(blocks[index], markdown, inherited.get(path),
                                       predecessor=blocks[index - 1] if index else None,
                                       inherited=True):
            exclusions.add((path, index))

    def sections(path, items, expected):
        for index, section in enumerate(items):
            owner = (*path, index)
            following = (expected[index + 1] if index + 1 < min(len(items), len(expected))
                         and items[index + 1]["id"] == expected[index + 1]["id"] else None)
            trailing(owner + ("body",), section["blocks"], following)
            sections(owner + ("sections",), section["children"],
                     expected[index]["children"] if index < len(expected) else ())

    items, expected = document["sections"], source["sections"]
    blocks = document.get("blocks", [])
    root = dict(id="document-overview", fragmentAliases=source.get("fragmentAliases", []))
    if source.get("heading") and blocks and anchor_block(blocks[0], markdown, anchor_lines(root)):
        exclusions.add((("root",), 0))
    following = expected[0] if items and expected and items[0]["id"] == expected[0]["id"] else None
    trailing(("root",), blocks, following)
    sections(("sections",), items, expected)
    return exclusions, carried


def document_signature(document, *, export=None, label=None, exclusions=()):
    """Project owners separately from their content and layout obligations."""
    axes = {key: [] for key in AXES if not key.startswith("rendered-")}
    retained = None if export is None else {"external", "email", "document"}
    if export == "addressable":
        retained |= {"section"}

    def record(path, kind, value, links, gap):
        axes["owner"].append((path, kind))
        axes["glyph"].append(re.sub(r"[ \t\n]", "", value))
        axes["author-space"].append((path, tuple(
            (i, ch) for i, ch in enumerate(value.replace("\n", "")) if ch in " \t")))
        axes["hard-row"].append((path, tuple(
            i for i, ch in enumerate(value) if ch == "\n")))
        axes["trailing-row"].append((path, len(value) - len(value.rstrip("\n"))))
        axes["gap"].append((path, gap))
        for start, end, target in links:
            axes["link-label"].append(value[start:end])
            axes["link-owner"].append((path, start, end))
            axes["target"].append(target)

    def physical_receiver(block):
        kind = block["type"]
        nodes = block.get("children", [])
        if kind == "paragraph":
            value, _ = inline_signature(nodes, retained_links=retained)
            return bool(value.strip("\n"))
        return kind == "preformatted" and any(
            node["type"] in ("text", "code", "line-break") for node in nodes
        )

    def content(path, blocks):
        index = 0
        pending_gap = 0
        preceding_content = False
        for block_index, block in enumerate(blocks):
            if (path, block_index) in exclusions:
                continue
            kind = block["type"]
            if kind == "vertical-space":
                if block["lines"]:
                    following = blocks[block_index + 1:block_index + 2]
                    if (export and preceding_content and block["lines"] == 1 and following
                            and (path, block_index + 1) not in exclusions
                            and physical_receiver(following[0])):
                        # Interior one-row spacing uses a CommonMark blank
                        # source line and belongs to the following block.
                        pending_gap = 1
                    else:
                        record((*path, index), kind, "", (), block["lines"])
                        index += 1
                    preceding_content = False
                continue
            if kind not in ("paragraph", "preformatted", "thematic-break"):
                raise Review(f"block {kind!r} needs its own projection rule")
            nodes = block.get("children", [])
            value, links = inline_signature(nodes, retained_links=retained)
            gap = max(pending_gap, block.get("layout", {}).get("spacingBeforeLines", 0))
            pending_gap = 0
            if not value and not links and not gap and (not nodes or kind == "paragraph"):
                if kind != "thematic-break":
                    continue
            if export and kind == "preformatted":
                if links:
                    raise Review("fenced-code link ownership is a separate projection")
                value = layout_rows(value, block.get("inlineLayout", {}).get("rowHints", []))
            if export and index == 0 and kind == "preformatted" and gap == 1:
                # At an owner start finish_boundaries spells the one
                # leading row as &#10; before the independent fence.
                record((*path, index), "paragraph", "\n", (), 0)
                index += 1
                gap = 0
            record((*path, index), kind, value, links, gap)
            preceding_content = (bool(value) or kind == "thematic-break"
                                 or kind == "preformatted" and physical_receiver(block))
            index += 1

    def sections(path, items):
        for index, section in enumerate(items):
            current = (*path, index)
            value, links = inline_signature(section["heading"]["content"], retained_links=(
                retained | {"manual"} if retained is not None else None))
            gap = section.get("spacingBeforeLines", 0)
            if export and path == ("sections",) and index == 0 and not document.get("blocks"):
                # The Markdown H1/H2 separator is framing; both native
                # title composition and its readback have a title gap.
                gap = max(1, gap)
            record(current, "heading", value, links, gap)
            content(current + ("body",), section["blocks"])
            sections(current + ("sections",), section["children"])

    if heading := document.get("heading"):
        value, links = inline_signature(heading["content"], retained_links=(
            retained | {"manual"} if retained is not None else None))
        record(("heading",), "heading", value, links, 0)
    elif export and label:
        # Native bibliographic casing can differ from the query label.
        # The export uses the latter only when no authored heading exists.
        record(("heading",), "heading", label, (), 0)
    content(("root",), document.get("blocks", []))
    sections(("sections",), document["sections"])
    # Glyphs and their owner topology are separate axes. A synthetic
    # empty framing owner must not look like added or lost body text.
    axes["glyph"] = "".join(axes["glyph"])
    return axes


def fidelity(expected, observed):
    """Return separate evidence for each axis; differences retain both values."""
    return {axis: ("exact",) if value == observed[axis] else ("difference", value, observed[axis])
            for axis, value in expected.items()}


def wire_document(observation):
    document = json.loads(observation["json"])["document"]
    if document is None:
        raise Review("no transported document")
    return document


def compare_direct(expected, observed):
    return fidelity(document_signature(wire_document(expected)),
                    document_signature(wire_document(observed)))


def readback(expected, observed, *, export, origin=None, previous=None):
    """Check the named stage against its own input, even outside native strict."""
    if expected is None or observed is None:
        return {axis: ("blocked",) for axis in AXES}
    states = {}
    try:
        bundle = json.loads(expected["json"])
        source = bundle["document"]
        if source is None:
            raise Review("no transported source document")
        actual = wire_document(observed)
        source_exclusions, actual_exclusions = (), ()
        if export == "addressable":
            native = wire_document(origin) if origin is not None else source
            prior = {}
            if previous is not None:
                previous_document = wire_document(previous)
                proven, prior = section_anchors(previous_document, previous.get("markdown"), native)
                if expected is previous:
                    source_exclusions = proven
                anchor_source = previous_document
            else:
                anchor_source = source
            actual_exclusions, _ = section_anchors(
                actual, observed.get("markdown"), anchor_source, prior,
            )
        states = fidelity(document_signature(source, export=export, label=bundle.get("label"),
                                             exclusions=source_exclusions),
                          document_signature(actual, exclusions=actual_exclusions))
    except (KeyError, Review) as error:
        states = {axis: ("review", str(error)) for axis in AXES if not axis.startswith("rendered-")}
    if export == "default" and all(states[axis][0] == "exact" for axis in ("owner", "gap")):
        # Title framing differs: the roff label may have a manual section
        # suffix, but the rest is an exact projection in this shape.
        first = expected["plain"].partition("\n")[2]
        second = observed["plain"].partition("\n")[2]
        states["rendered-row"] = (("exact",) if first == second else ("difference", first, second))
        # Content-bearing rows have a separate obligation, independent of
        # heading framing and paragraph gaps. Spaces within them are exact.
        old, new = (tuple(row for row in text.split("\n") if row) for text in (first, second))
        states["rendered-content-row"] = (("exact",) if old == new else ("difference", old, new))
    else:
        for axis in ("rendered-row", "rendered-content-row"):
            states[axis] = ("review", "unmapped renderer framing")
    return states


def chain_obligations(observations):
    output = {}
    for route in ("default", "addressable"):
        first, second = f"{route}-1", f"{route}-2"
        output[first] = {"from-source": readback(observations.get("roff"), observations.get(first),
                                                export=route)}
        output[second] = {
            "from-source": readback(observations.get("roff"), observations.get(second), export=route,
                                    origin=observations.get("roff"), previous=observations.get(first)),
            "from-first": readback(observations.get(first), observations.get(second), export=route,
                                   origin=observations.get("roff"), previous=observations.get(first)),
        }
    return output
