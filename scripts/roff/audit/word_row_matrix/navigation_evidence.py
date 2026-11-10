"""Exact source and byte-span evidence for generated Markdown navigation."""

import re


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
