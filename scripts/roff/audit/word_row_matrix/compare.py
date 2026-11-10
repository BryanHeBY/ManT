"""Strict comparison for the bounded prose lane, with explicit unresolved layouts."""
from base64 import b64decode
import re
import unicodedata

from .core import MDOC
from .followup import HORIZ, MODE, PREFIX, operand

# The four core word generators have a fixed prose origin.
STRICT = {"A-atomic", "A-phase", "A-rearm", "A-seam"}
HEADING = "DESCRIPTION"


class UnsupportedExtraction(ValueError):
    """Preserve raw evidence instead of guessing a furniture or cell range."""


def device_row(raw):
    """Decode the native scalar stream, refusing unsupported column replacements."""
    cells = []
    backspaces = []
    column = 0
    for offset, character in enumerate(raw):
        if character == "\b":
            if column == 0:
                raise UnsupportedExtraction("backspace before the first device cell")
            backspaces.append((offset, column - 1))
            column -= 1
            continue
        kind = unicodedata.category(character)
        if kind in ("Mn", "Me"):
            base = column - 1
            while base >= 0 and cells[base] is None:
                base -= 1
            if base < 0:
                raise UnsupportedExtraction("isolated combining scalar")
            cells[base] += character
            continue
        if kind.startswith("C"):
            raise UnsupportedExtraction("unsupported terminal control")
        width = 2 if unicodedata.east_asian_width(character) in ("W", "F") else 1
        if column == len(cells):
            cells.extend([character, *([None] * (width - 1))])
        elif (cells[column] is None or width != 1
              or column + 1 < len(cells) and cells[column + 1] is None):
            raise UnsupportedExtraction("overstrike intersects a wide native glyph")
        else:
            cells[column] = character
        column += width
    return "".join(cell for cell in cells if cell is not None), backspaces


def native_prose(receipt, tree):
    """Remove only the known DESCRIPTION prefix and the exact CVS footer."""
    if receipt["code"] != 0 or not receipt["stdout"]["utf8"]:
        raise UnsupportedExtraction("no certified native UTF-8 output")
    if not re.search(rb"^ +DESCRIPTION \(text\) \d+:\d+$", tree, re.MULTILINE):
        raise UnsupportedExtraction("description origin absent in native tree")
    raw = b64decode(receipt["stdout"]["base64"], validate=True).decode()
    bold = "".join(character + "\b" + character for character in HEADING)
    marker = "\n" + bold + "\n"
    if raw.count(marker) != 1:
        raise UnsupportedExtraction("description furniture is not unique")
    start = raw.index(marker) + len(marker)
    # print_mdoc_foot() emits a spacing row, OS, centered fixed prologue
    # date, and the same title/section as the header. At width 160 this
    # footer fits on one line in the certified Linux profile.
    footer = re.search(r"\nLinux [^\n]+ +November 10, 2026 +CHECK\(1\)\n\Z", raw)
    if footer is None or footer.start() < start:
        raise UnsupportedExtraction("unrecognized fixed-template footer")
    body = raw[start:footer.start()]
    if body and not body.endswith("\n"):
        raise UnsupportedExtraction("unterminated native body")
    spans = [(0, len(raw[:start].encode()), "description-prefix"),
             (len(raw[:footer.start()].encode()), len(raw.encode()), "mdoc-footer")]
    rows = []
    mapping = []
    for raw_row in body.split("\n")[:-1]:
        row, advances = device_row(raw_row)
        if row and not row.startswith(" " * 5):
            raise UnsupportedExtraction("cannot prove a five-cell MDOC origin")
        rows.append(row[5:] if row else "")
        mapping.append(advances)
    return dict(rows=rows, furnitureSpans=spans, overstrikes=mapping)


def product_prose(plain):
    marker = "\n" + HEADING + "\n"
    if plain.count(marker) != 1:
        raise UnsupportedExtraction("portable description origin is not unique")
    body = plain.split(marker, 1)[1]
    # The library has no CLI framing newline. Flow::finish_rows supplies
    # one delimiter only for a completed tail, after the real empty rows.
    return body.removesuffix("\n").split("\n") if body else []


def tree_nodes(tree):
    """Decode the native preorder, retaining source coordinates and raw flags."""
    nodes = []
    stack = []
    pattern = re.compile(
        r"^( *)(.*?) \((elem|text|block|head|body|tail|tbl|eqn|comment)\) ([(*]*)(\d+):(\d+)(.*)$"
    )
    roots = 0
    for line in tree.decode().split("\n"):
        match = pattern.match(line)
        if not match:
            continue
        space, label, kind, flags, row, column, suffix = match.groups()
        indent = len(space)
        # For TEXT, indentation beyond the owner's known child indent
        # is part of the text spelling, including an authored leading SP.
        while stack and (stack[-1]["indent"] >= indent
                         or kind == "text" and stack[-1]["childIndent"] > indent):
            stack.pop()
        if stack:
            parent = stack[-1]
            path = [*parent["path"], parent["children"]]
            parent["children"] += 1
            if kind == "text":
                label = " " * (indent - parent["childIndent"]) + label
                indent = parent["childIndent"]
        else:
            path = [roots]
            roots += 1
        node = dict(path=path, kind=kind, label=label, line=int(row), column=int(column),
                    lineStart="*" in flags, noFill="NOFILL" in suffix.split(),
                    noPrint="NOPRT" in suffix.split(),
                    indent=indent, childIndent=indent + (2 if kind == "block" else 4), children=0)
        nodes.append(node)
        stack.append(node)
    return nodes


def group_witness(tree):
    """Enumerate every alignment request and each direct child in native preorder."""
    nodes = tree_nodes(tree)
    groups = []
    by_parent = {}
    for node in nodes:
        by_parent.setdefault(tuple(node["path"][:-1]), []).append(node)
    for node in nodes:
        if node["kind"] == "elem" and node["label"] in ("ce", "rj"):
            children = by_parent.get(tuple(node["path"]), [])
            groups.append(dict(node=node, occurrence=len(groups), children=children,
                               starts=[i for i, child in enumerate(children[1:], 1)
                                       if i == 1 or child["kind"] == "text" and child["lineStart"]]))
    return groups


def word_comparison(source, expected, actual):
    """Classify only the source-proven device differences in W-core.

    Keep the unmodified row evidence. These are Review, not exact passes:
    deep rewinds may change an already authored ordinary cell, and the
    literal device's temporary-origin update is not a supported IR action.
    """
    if expected == actual:
        return "exact"
    body = source.removeprefix(MDOC)
    mode = next((i for i, (enter, _) in enumerate(MODE) if enter and body.startswith(enter)), 0)
    body = body.removeprefix(MODE[mode][0])
    first, _, later = body.partition("\n")
    second = later.partition("\n")[0]
    # The frozen reference proves the thresholds in both units. Do not
    # infer a motion size from a changed row or from the spelling alone.
    motion = next((i for i, entry in enumerate(HORIZ[2:], 2) if entry in second), None)
    units = {2: 24, 3: 48, 4: 72, 5: 24, 6: 25, 7: 47, 8: 48}.get(motion, 0)
    # term_word divides by the native cell width for a buffered rewind.
    # If it moves the origin instead, ascii_advance rounds at half a cell.
    columns, origin = units // 24, (units + 11) // 24
    if (mode == 3 and origin and expected and actual and expected[0] == actual[0]
            and len(expected) == len(actual)
            and all(a == b or (b and a == " " * origin + b)
                    for a, b in zip(expected[1:], actual[1:]))):
        # term.c:709-714 moves offset/ti on an empty input row. Exact
        # source whitespace and the accepted rows still match after this
        # one proven origin; a mismatch elsewhere must remain a difference.
        return "review-literal-origin"
    if (first + "\n" in (operand(".No", PREFIX[0]), operand(".No", PREFIX[1]))
            and (mode in (0, 2) and columns == 3 or mode == 1 and columns in (2, 3))):
        # The unsupported cursor can overwrite a suffix of ALPHA and
        # reject further native cells. It cannot remove the still-accepted
        # prefix, the explicit tail word, or any surviving BETA text.
        if (actual and actual[0].startswith("ALP")
                and all(not any(word in row for row in expected)
                        or any(word in row for row in actual)
                        for word in ("BETA", "OMEGA"))):
            return "review-protected-cell"
    return "difference"


def check_rewind(source, tree, receipt, observation):
    if observation is None:
        return dict(state="blocked")
    if not all(observation[key] for key in ("jsonEquivalent", "validDocument", "ansiEquivalent")):
        return dict(state="public-invariant-failure")
    try:
        native = native_prose(receipt, tree)["rows"]
    except UnsupportedExtraction as error:
        return dict(state="review-extractor", reason=str(error))
    try:
        actual = product_prose(observation["plain"])
    except UnsupportedExtraction as error:
        return dict(state="difference", native=native, reason=str(error))
    state = word_comparison(source, native, actual)
    return dict(state=state) if state == "exact" else dict(state=state, native=native, actual=actual)
