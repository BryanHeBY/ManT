"""Raw physical-row and source facts shared by the ordered section lanes."""

from .compare import UnsupportedExtraction
from .section_observations import exact, inline_value


HEAD_IDS = {
    "NEXT": "next",
    r"\&": "section", r"\p": "p", r"\p NEXT": "p-next", r"\&\p NEXT": "p-next",
    r"NEXT\zX": "next-zx", r"\zNEXT": "znext", r"NEXT\p WORD": "next-p-word",
    r"NEXT\p \p WORD": "next-p-p-word",
    r"\h'-1n'NEXT": "h-1n-next", r"\h'-2n'NEXT": "h-2n-next",
}


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


def suffix_blanks(rows):
    return len(rows) - 1 - max((index for index, row in enumerate(rows) if row), default=-1)


def inline_owners(nodes, kind):
    for node in nodes:
        if node["type"] == kind:
            yield node
        yield from inline_owners(node.get("children", ()), kind)


def block_rows(block):
    kind = block["type"]
    if kind == "vertical-space":
        return [""] * block["lines"]
    if kind not in ("paragraph", "preformatted"):
        raise UnsupportedExtraction(f"unmapped product block: {kind}")
    children = block["children"]
    value = inline_value(children)
    # A paragraph's last break opens a row that joins the next block.
    # A literal keeps its physical tail, including authored empty Text
    # inside formatting or link wrappers. Navigation-only roots have no row.
    if kind == "paragraph" and value:
        return value.removesuffix("\n").split("\n")
    if kind == "preformatted" and (value or next(inline_owners(children, "text"), None)):
        return value.split("\n")
    return []


def projected_rows(blocks):
    return [row for block in blocks for row in block_rows(block)]


def payload_source_axis(blocks, line):
    """Check every payload-bearing owner, not just the first matching block."""
    actual = [(block.get("source") or {}).get("line") for block in blocks if any(block_rows(block))]
    # Independent gaps and empty rows carry no payload to attribute. The
    # old word and following BODY witness in this lane must each survive.
    return exact([line] * max(1, len(actual)), actual)
