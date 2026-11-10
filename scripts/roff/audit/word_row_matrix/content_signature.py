"""Independent per-owner text, layout, and target projections for the audit."""

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
