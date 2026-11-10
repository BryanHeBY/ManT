"""Source actions and public section/row witnesses for the bounded section matrix."""

from base64 import b64decode
from collections import defaultdict
import json
import re

from .compare import UnsupportedExtraction, device_row, tree_nodes

MACROS = {"SH", "SS", "Sh", "Ss"}
TRANSPARENT = set("ft ll mc po ta Db Es Sm Tg DT UC PD AT".split())
NAMED_HEADING = re.compile(r"^ *(?P<name>NEXT|EXT)$")
BODY_MARKER = re.compile(r"^ *AFTERMARK$")
PRIOR_MARKER = re.compile(r"^ *(FIRST|PREVIOUS|ALPHA|BEFOREMARK|DESCRIPTION|NAME|EMPTY[1-4]|X)$")
PRIOR_WORDS = {
    "ALPHA": "ALPHA", r"ALPHA\c": "ALPHA", "BEFOREMARK": "BEFOREMARK",
    r"BEFOREMARK\c": "BEFOREMARK", r"BEFOREMARK\z": "BEFOREMARK", r"\zX": "X",
}
INVISIBLE_GROUPS = (r"\&", r"\p\&", r"\& \&", r"\p\& \p\&")


def transparent(node):
    return (node["kind"] == "comment" or node["noPrint"]
            or node["kind"] != "text" and node["label"] in TRANSPARENT)


def branch_witness(tree):
    nodes = tree_nodes(tree)
    children = defaultdict(list)
    for node in nodes:
        children[tuple(node["path"][:-1])].append(node)

    def part(node, kind):
        return next((child for child in children[tuple(node["path"])] if child["kind"] == kind), None)

    def content(node):
        return children[tuple(node["path"])] if node else []

    matches = [node for node in nodes if node["kind"] == "block" and node["label"] in MACROS
               and [text["label"] for text in content(part(node, "head"))] == ["NEXT"]]
    if len(matches) != 1:
        raise UnsupportedExtraction("normalized tree has no unique NEXT owner")
    target = matches[0]
    siblings = children[tuple(target["path"][:-1])]
    prior = siblings[:siblings.index(target)]
    predecessor = next((node for node in reversed(prior) if not transparent(node)), None)
    raw = content(part(predecessor, "body")) if predecessor else []
    filtered = [node for node in raw if not transparent(node)]
    peer = predecessor is not None and predecessor["label"] == target["label"]
    before = nodes[:nodes.index(target)]
    pd = 1
    for node in before:
        if node["kind"] == "elem" and node["label"] == "PD":
            args = content(node)
            pd = int(args[0]["label"]) if args else 1
    macro = target["label"]
    if macro in ("SH", "SS"):
        action = "paragraph-distance" if predecessor and (not peer or filtered) else "none"
        requested = pd if action == "paragraph-distance" else 0
    elif macro == "Sh":
        action = "vertical-row" if not peer or raw else "none"
        requested = int(action == "vertical-row")
    else:
        action = "vertical-row" if predecessor else "newline"
        requested = int(predecessor is not None)
    requests = [dict(node=node, arguments=content(node)) for node in before
                if node["kind"] == "elem" and node["label"] == "sp" and not transparent(node)]
    # This generator puts at most one negative request immediately before the
    # target (possibly before an Ed post). Read its admitted AST, not its input
    # template. No intervening term_word is generated in this domain.
    debt = sum(-int(req["arguments"][0]["label"]) for req in requests
               if req["arguments"] and req["arguments"][0]["label"] in ("-1", "-2"))
    body = part(target, "body")
    marker_lines = ([node["line"] for node in nodes
                     if node["path"][:len(body["path"])] == body["path"]
                     and node["kind"] == "text" and node["label"] == "AFTERMARK"]
                    if body else [])
    words = [node for node in before if node["kind"] == "text" and not node["noPrint"]
             and node["label"] in PRIOR_WORDS]
    prior_word = None
    if len(words) == 1:
        word = words[0]
        enclosing = [node for node in before if node["kind"] == "block" and node["label"] in MACROS
                     and word["path"][:len(node["path"])] == node["path"]]
        if enclosing:
            prior_word = dict(value=PRIOR_WORDS[word["label"]], node=word,
                              section=enclosing[-1], mayReachHeading=word["label"] == r"\zX")
    repeated = [node for node in prior if node["kind"] == "text"]
    invisible_group = None
    if (macro == "SS" and pd == 0 and repeated
            and sum(len(node["label"].split(" ")) for node in repeated) > 1
            and all(node["label"] in INVISIBLE_GROUPS for node in repeated)
            and all(node["kind"] == "text" or node["label"] in ("PD", "nf") for node in prior)):
        enclosing = [node for node in before if node["kind"] == "block" and node["label"] == "SH"
                     and target["path"][:len(node["path"])] == node["path"]]
        if enclosing:
            # print_man_node() calls term_newln() only for actual no-fill
            # NODE_LINE events. These operands cannot arm continuation;
            # the last active input instead reaches post_SH(HEAD).
            prior_rows = sum(left["noFill"] and right["noFill"] and right["lineStart"]
                             for left, right in zip(repeated, repeated[1:]))
            invisible_group = dict(section=enclosing[-1], priorRows=prior_rows,
                                   sourceLines=[node["line"] for node in repeated])
    return dict(node=target, previous=predecessor, predecessorChildren=content(predecessor),
                rawBodyChildren=raw,
                filteredBodyChildren=filtered, requests=requests, pd=pd, action=action,
                bodyChildren=content(body), bodyMarkerLines=marker_lines, priorWord=prior_word,
                invisibleGroup=invisible_group,
                expectedGap=max(0, requested - debt))


def sections(items):
    for section in items:
        yield section
        yield from sections(section["children"])


def inline_value(nodes):
    return "".join(node.get("value", "\n" if node["type"] == "line-break" else
                            inline_value(node.get("children", ()))) for node in nodes)


def row_region(rows, shared_prefix=None):
    shared_heading = (re.compile(rf"^ *{re.escape(shared_prefix)} *(?P<name>NEXT|EXT)$")
                      if shared_prefix is not None else None)
    headings = []
    for index, row in enumerate(rows):
        if match := NAMED_HEADING.fullmatch(row):
            headings.append((index, match["name"], None))
        elif shared_heading and (match := shared_heading.fullmatch(row)):
            headings.append((index, match["name"], shared_prefix))
    if len(headings) != 1:
        return None
    index, name, shared = headings[0]
    first = next((i for i in range(index - 1, -1, -1) if rows[i]), None)
    after = next((i for i in range(index + 1, len(rows)) if BODY_MARKER.fullmatch(rows[i])), None)
    return dict(heading=name, shared=shared, index=index,
                prior=rows[first] if first is not None else None,
                before=rows[first + 1:index] if first is not None else rows[:index],
                after=None if after is None else rows[index + 1:after])


def exact(want, got):
    return ("exact",) if want == got else ("difference", want, got)


def matching_block_rows(blocks, word):
    return [(block, row) for block in blocks
            for row in inline_value(block.get("children", ())).split("\n")
            if row.strip(" ") == word]


def between_markers(rows):
    first = [index for index, row in enumerate(rows) if row.strip(" ") == "FIRST"]
    after = [index for index, row in enumerate(rows) if BODY_MARKER.fullmatch(row)]
    if len(first) == len(after) == 1 and first[0] < after[0]:
        return rows[first[0] + 1:after[0]]
    return None


def section_marker(row):
    match = PRIOR_MARKER.fullmatch(row or "")
    return match[1] if match else None


def invisible_owner_axis(group, native_rows, owners, heading_rows):
    owner = next((section for section in owners
                  if section.get("source", {}).get("line") == group["section"]["line"]), None)
    if not owner or len(native_rows) < group["priorRows"]:
        return ("review", "invisible source interval has no comparable prior section")
    prior_rows = 0
    for block in owner["blocks"]:
        children = block.get("children", ())
        row = inline_value(children)
        if (block["type"] != "preformatted" or not children or row.strip(" \n")
                or block.get("source", {}).get("line") not in group["sourceLines"]):
            return ("difference", "source-owned invisible literal rows", block)
        prior_rows += row.count("\n") + int(not row.endswith("\n"))
    expected = group["priorRows"]
    return exact((expected, len(native_rows) - expected), (prior_rows, heading_rows))


def prior_word_axes(witness, native_rows, native, actual_rows, actual, owners, target, heading):
    word = witness["priorWord"]
    if not word or not native:
        return {}, heading
    value = word["value"]
    if not (native["shared"] == value
            or any(row.strip(" ") == value for row in native_rows[:native["index"]])):
        return {"prior-content": ("review", "native prior word has no comparable marker row")}, heading
    owner = next((section for section in owners
                  if section.get("source", {}).get("line") == word["section"]["line"]), None)
    carriers = matching_block_rows(owner["blocks"], value) if owner else []
    # A buffered zero-advance glyph can enter the HEAD's first native word.
    # Ordinary prior words are never absorbed into the semantic HEAD even
    # if native SS geometry places them on the same device row.
    prefix = word["mayReachHeading"] and native["shared"] == value and heading.startswith(value)
    if prefix:
        heading = heading.removeprefix(value).removeprefix(" ")
    source_owned = sum(word["section"]["line"] < block.get("source", {}).get("line", 0)
                       <= word["node"]["line"] for block, _ in carriers)
    in_new_body = len(matching_block_rows(target["blocks"], value))
    rendered = (sum(row.strip(" ") == value for row in actual_rows[:actual["index"]])
                + int(actual["shared"] == value)) if actual else 0
    return {
        "prior-content": exact((1, 0), (len(carriers) + int(prefix), in_new_body)),
        "prior-source": exact(1, source_owned + int(prefix)),
        "prior-rendered": exact(1, rendered),
    }, heading


def check_section(tree, utf8, observation):
    """Return per-axis facts, never substituting a route for the original IR."""
    try:
        witness = branch_witness(tree)
        if utf8["code"] != 0 or not utf8["stdout"]["utf8"]:
            raise UnsupportedExtraction("native UTF-8 result unavailable")
        native_rows = [device_row(line)[0] for line in
                       b64decode(utf8["stdout"]["base64"], validate=True).decode().split("\n")[:-1]]
    except UnsupportedExtraction as error:
        return dict(state="uncovered", reason=str(error))
    if observation is None:
        return dict(state="error", witness=witness, reason="missing direct observation")
    document = json.loads(observation["json"])["document"]
    owners = list(sections(document["sections"]))
    target = next((node for node in owners
                   if node.get("source", {}).get("line") == witness["node"]["line"]), None)
    if target is None:
        return dict(state="difference", witness=witness, reason="missing source section owner")
    actual_rows = observation["plain"].split("\n")
    word = witness["priorWord"]
    native = row_region(native_rows, word["value"] if word else None)
    actual = row_region(actual_rows, word["value"] if word and word["mayReachHeading"] else None)
    axes = {
        "entry-gap": exact(witness["expectedGap"], target.get("spacingBeforeLines", 0)),
        "transport": exact(True, all(observation[key] for key in
                                     ("validDocument", "jsonEquivalent", "ansiEquivalent"))),
    }
    # SS may include one generated leading field space. Ordinary source
    # words in its neighboring owners are checked separately, unmodified.
    heading = inline_value(target["heading"]["content"]).removeprefix(" ")
    prior, heading = prior_word_axes(witness, native_rows, native, actual_rows, actual,
                                     owners, target, heading)
    axes.update(prior)
    replayed_empty_body = any(block["type"] == "preformatted" and block.get("source") is None
                              for block in target["blocks"])
    comparable_before = (native and actual and section_marker(native["prior"])
                         and section_marker(native["prior"]) == section_marker(actual["prior"]))
    if native:
        rows = heading.split("\n")
        if (comparable_before and len(rows) > 1 and len(rows) - 1 <= len(native["before"])
                and all(not row for row in native["before"])
                and all(not row.strip(" ") for row in rows[:-1])):
            # These are independent completed rows, not title characters.
            # The exact before-heading interval still tests their number.
            heading = rows[-1]
        if actual and actual["heading"] == native["heading"]:
            # Source HEAD is exactly NEXT. Spaces after a proven retained
            # invisible input cell are native positioning, not author text.
            heading = heading.lstrip(" ")
        axes["heading"] = exact(native["heading"], heading)
    else:
        blank = between_markers(native_rows)
        body = [(node["kind"], node["label"]) for node in witness["bodyChildren"]]
        if blank and all(not row for row in blank) and body == [("text", "AFTERMARK")]:
            # The fixed printer can reject the whole shared NEXT field.
            # Its blank interval is real even though no heading glyph prints.
            # Bind it to native FIRST/AFTERMARK before consulting ManT.
            axes["heading"] = exact("", heading.replace("\n", ""))
            axes["suppressed-heading-rows"] = exact(blank, between_markers(actual_rows))
            resolved_gap = target.get("spacingBeforeLines", 0)
            has_heading_endline = "\n" in heading or resolved_gap > witness["expectedGap"]
            axes["suppressed-heading-owner"] = exact(True, has_heading_endline)
            axes["body-owner"] = exact(False, replayed_empty_body)
            if resolved_gap > witness["expectedGap"]:
                # This public owner can carry both BLOCK pre spacing and
                # a HEAD post endline. The exact native interval above is
                # measurable, but their individual IR counts are not.
                axes["entry-gap"] = ("review", "suppressed HEAD endline shares resolved spacing")
        else:
            axes["heading"] = ("review", "native heading has no comparable marker row")
    if native and not actual:
        axes["rendered-heading"] = ("difference", native["heading"], None)
    if native and actual and native["after"] is not None:
        axes["after-heading"] = exact(native["after"], actual["after"])
    if comparable_before:
        axes["before-heading"] = exact(native["before"], actual["before"])
        if witness["invisibleGroup"] and all(not row for row in native["before"]):
            axes["invisible-owner"] = invisible_owner_axis(
                witness["invisibleGroup"], native["before"], owners, len(rows) - 1)
    if native and native["after"] == []:
        axes["body-owner"] = exact(False, replayed_empty_body)
    if any(BODY_MARKER.fullmatch(row) for row in native_rows):
        carriers = matching_block_rows(target["blocks"], "AFTERMARK")
        axes["body-content"] = exact(1, len(carriers))
        if carriers and witness["bodyMarkerLines"]:
            axes["body-source"] = exact(True, any(
                witness["node"]["line"] < block.get("source", {}).get("line", 0)
                <= max(witness["bodyMarkerLines"]) for block, _ in carriers))
    for axis in ("before-heading", "after-heading", "body-owner", "body-content", "body-source",
                 "prior-content", "prior-source", "prior-rendered", "suppressed-heading-rows",
                 "suppressed-heading-owner", "invisible-owner"):
        axes.setdefault(axis, ("review", "no independently comparable marker interval"))
    state = ("difference" if any(row[0] == "difference" for row in axes.values())
             else "asserted-axes-pass")
    return dict(state=state, witness=witness, axes=axes, native=native, actual=actual)
