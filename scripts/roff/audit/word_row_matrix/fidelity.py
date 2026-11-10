"""Independent source-to-readback checks for the bounded Markdown lane.

Rows and owner paths are compared without trimming. The unsupported shapes
are explicit obligations, not passes inferred from two equal readbacks.
"""
import json
import re

AXES = ("glyph", "author-space", "hard-row", "owner", "owner-content", "link-label",
        "link-owner", "target", "gap", "trailing-row", "rendered-row", "rendered-content-row")
STRUCTURAL_AXES = tuple(axis for axis in AXES if not axis.startswith("rendered-"))


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
    # Admit only simple IDs the exporter emits verbatim, including the
    # Unicode letters and numbers in Markdown-generated heading IDs.
    if any(not value or any(not (ch.isalnum() or ch in "_.:-") for ch in value)
           for value in ids):
        return ()
    return tuple(f'<a id="{value}"></a>' for value in ids)


def syntax_adjacent(markdown, before, after):
    """Prove a generated marker is on the intended heading boundary."""
    left = (before.get("source") or {}).get("byteRange")
    right = (after.get("source") or {}).get("byteRange")
    if not left or not right:
        return False
    end, start = left["end"], right["start"]
    # The parser may count the first newline in the preceding span. No
    # other text or a run of authored blank lines can prove this boundary.
    return (0 <= left["start"] < end <= start < right["end"] <= len(markdown)
            and markdown[end:start] in (b"\n", b"\n\n"))


def anchor_block(block, markdown, lines, *, predecessor=None, inherited=False):
    """Prove a standalone marker's spelling, span and source-owned identity."""
    if not lines or markdown is None or block["type"] != "paragraph":
        return False
    span = (block.get("source") or {}).get("byteRange")
    if not span or block["children"] != [{"type": "text", "value": " ".join(lines)}]:
        return False
    if not 0 <= span["start"] < span["end"] <= len(markdown):
        return False
    if block.get("inlineLayout"):
        return False
    gap = 0
    if predecessor is not None:
        previous = (predecessor.get("source") or {}).get("byteRange")
        if not previous or not 0 <= previous["start"] < previous["end"] <= span["start"]:
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


def exact_generated_title(markdown, heading, label):
    """The bounded fallback H1 is a simple, unescaped query label."""
    span = (heading.get("source") or {}).get("byteRange")
    if not span or not label or not re.fullmatch(r"[a-zA-Z0-9_-]+", label):
        return False
    if heading["content"] != [{"type": "text", "value": label}]:
        return False
    start, end = span["start"], span["end"]
    spelling = f"# {label}".encode()
    return start == 0 and 0 < end <= len(markdown) and markdown[:end] in (spelling, spelling + b"\n")


def section_anchors(document, markdown, source, inherited=None, *, label=None):
    """Return proven block exclusions and their per-boundary source spellings."""
    exclusions, carried = set(), {}
    if markdown is None:
        return exclusions, carried
    markdown = markdown.encode()
    inherited = inherited or {}

    def trailing(path, blocks, following, target):
        if target is None or not blocks:
            return False
        index = len(blocks) - 1
        lines = anchor_lines(target)
        heading = following["heading"]
        if (not syntax_adjacent(markdown, blocks[index], heading)
                or not anchor_block(blocks[index], markdown, lines,
                                    predecessor=blocks[index - 1] if index else None)):
            return False
        exclusions.add((path, index))
        carried[path] = lines
        index -= 1
        if (index >= 0
                and syntax_adjacent(markdown, blocks[index], blocks[index + 1])
                and anchor_block(blocks[index], markdown, inherited.get(path),
                                 predecessor=blocks[index - 1] if index else None,
                                 inherited=True)):
            exclusions.add((path, index))
        return True

    def preorder(path, items):
        for index, section in enumerate(items):
            owner = (*path, index)
            yield owner, section
            yield from preorder(owner + ("sections",), section["children"])

    # The source boundary before a heading belongs to the preceding owner
    # in preorder, including parent → child and last descendant → sibling.
    items = list(preorder(("sections",), document["sections"]))
    expected = dict(preorder(("sections",), source["sections"]))

    blocks = document.get("blocks", [])
    root = dict(id="document-overview", fragmentAliases=source.get("fragmentAliases", []))
    heading = document.get("heading")
    if (source.get("heading") and heading and blocks
            and syntax_adjacent(markdown, blocks[0], heading)
            and anchor_block(blocks[0], markdown, anchor_lines(root))):
        exclusions.add((("root",), 0))
        carried[("heading",)] = anchor_lines(root)
        # The next export escapes the first readback's root marker as an
        # ordinary paragraph. Require the already-proven first-round marker
        # and its exact new span after the heading, before any authored body.
        if (len(blocks) > 1 and syntax_adjacent(markdown, heading, blocks[1])
                and anchor_block(blocks[1], markdown, inherited.get(("heading",)),
                                 predecessor=blocks[0], inherited=True)):
            exclusions.add((("root",), 1))
    elif (not source.get("heading") and (source.get("blocks") or source.get("fragmentAliases"))
          and heading and blocks and exact_generated_title(markdown, heading, label)
          and syntax_adjacent(markdown, heading, blocks[0])
          and anchor_block(blocks[0], markdown, anchor_lines(root))):
        # render_markdown_with_context emits a root destination after its
        # fallback H1 when the source has root owners, even if those owners
        # have no Markdown projection. Its raw span, title and slot must
        # all agree before omitting it from source-owned glyphs.
        exclusions.add((("root",), 0))
        carried[("heading",)] = anchor_lines(root)

    # Parsed headings can acquire a new slug while the export still carries
    # the source ID. A marker at that exact heading boundary can prove this
    # owner correspondence; matching text or a matching slug alone is not
    # required for that evidence.
    owner, blocks = ("root",), blocks
    for path, section in items:
        target = expected.get(path)
        if owner is not None and trailing(owner, blocks, section, target):
            owner, blocks = path + ("body",), section["blocks"]
        elif target is not None and section["id"] == target["id"]:
            owner, blocks = path + ("body",), section["blocks"]
        else:
            owner = None
    return exclusions, carried


def document_signature(document, *, export=None, label=None, exclusions=(), direct=False):
    """Record normalized topology, its payloads, and independent layout evidence."""
    axes = {key: [] for key in STRUCTURAL_AXES}
    retained = None if export is None else {"external", "email", "document"}
    if export == "addressable":
        retained |= {"section"}

    def record(path, kind, value, links, gap, *, content_path=None):
        axes["owner"].append((path, kind))
        glyphs = re.sub(r"[ \t\n]", "", value)
        axes["glyph"].append(glyphs)
        # Use the normalized owner path after applying the known export
        # mappings below. Keep spaces and row boundaries on their own axes,
        # and do not fold Unicode scalar spellings or combine adjacent owners.
        if not direct or kind != "vertical-space":
            axes["owner-content"].append((content_path if direct and content_path is not None
                                         else path, kind, glyphs))
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
        owner_index = 0
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
            content_path = None
            if direct:
                content_path = (*path, owner_index)
                owner_index += 1
            if not value and not links and not gap and (not nodes or kind == "paragraph"):
                if kind != "thematic-break":
                    if direct:
                        # An empty IR owner cannot absorb another owner's
                        # text. Unlike an export, direct comparison has no
                        # reason to elide it from the correspondence.
                        axes["owner-content"].append((content_path, kind, ""))
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
            record((*path, index), kind, value, links, gap, content_path=content_path)
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


def fidelity(expected, observed, *, projected=False):
    """Return separate evidence for each axis; differences retain both values."""
    result = {
        axis: ("exact",) if value == observed[axis] else ("difference", value, observed[axis])
        for axis, value in expected.items()
    }
    if projected and result["owner"][0] != "exact":
        # No positional owner correspondence across an unaccounted-for
        # export shape. Keep the topology and glyph evidence, but don't claim
        # per-owner fidelity (or attribute arbitrary mismatches to owners).
        result["owner-content"] = ("review", "unmapped Markdown owner topology")
    return result


def wire_document(observation):
    document = json.loads(observation["json"])["document"]
    if document is None:
        raise Review("no transported document")
    return document


def compare_direct(expected, observed):
    if expected is None or observed is None:
        return {axis: ("blocked",) for axis in STRUCTURAL_AXES}
    try:
        return fidelity(document_signature(wire_document(expected), direct=True),
                        document_signature(wire_document(observed), direct=True))
    except (KeyError, Review) as error:
        return {axis: ("review", str(error)) for axis in STRUCTURAL_AXES}


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
            native_bundle = json.loads(origin["json"]) if origin is not None else bundle
            native = native_bundle["document"]
            prior = {}
            if previous is not None:
                previous_document = wire_document(previous)
                proven, prior = section_anchors(previous_document, previous.get("markdown"), native,
                                                label=native_bundle.get("label"))
                if expected is previous:
                    source_exclusions = proven
                anchor_source = previous_document
            else:
                anchor_source = source
            actual_exclusions, _ = section_anchors(
                actual, observed.get("markdown"), anchor_source, prior, label=bundle.get("label"),
            )
        states = fidelity(document_signature(source, export=export, label=bundle.get("label"),
                                             exclusions=source_exclusions),
                          document_signature(actual, exclusions=actual_exclusions), projected=True)
    except (KeyError, Review) as error:
        states = {axis: ("review", str(error)) for axis in STRUCTURAL_AXES}
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
