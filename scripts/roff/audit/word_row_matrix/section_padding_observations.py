"""Source-bound checks for reading padding versus actual completed rows."""

from base64 import b64decode
import json
import re

from .compare import UnsupportedExtraction, device_row, tree_nodes
from .section_facts import (
    HEAD_IDS, between_sections, inline_owners, payload_source_axis, projected_rows,
    section_node, suffix_blanks,
)
from .section_observations import exact, inline_value, sections
from .section_padding import operand


HEADS = dict(HEAD_IDS, LATER="later", **{r"\z": "z"})
HEAD_ROW = re.compile(r"NEXTX?|EXT|WORD|LATER")
OLD_ROW = re.compile(
    r"[ \t\u00a0]*(?:(?:ALPHA|BETA|GAMMA|X)[ \t\u00a0]*)*"
    r"(?:<(?:https://audit\.test/|audit@example\.test)>[ \t\u00a0]*)?"
)
PURE_MOTION = re.compile(
    r"(?:\\f[BI])?ALPHA\\p((?: +\\p\\&)+)(?:\\fR)? +((?:\\h'[12]n')+)"
    r"( +\\zX)?(?:\\c)*"
)
POST_LINK_MOTION = re.compile(r"\\h'[12]n'")
WRAPPED_OLD_WORD = re.compile(r"ALPHA\\p((?: +\\p\\&)+)(?: (\\h'[12]n'(?: BETA)?|\\ ))?")
KNOWN_FOOTER = re.compile(r"^.* +2026-11-10 +CHECK\(1\)$")


def source_witness(source, tree):
    nodes = tree_nodes(tree)
    first = section_node(nodes, "FIRST")
    if ".SH ENDTEST\n" in source:
        end = section_node(nodes, "ENDTEST")["line"]
    else:
        end = len(source.split("\n"))
    boundaries = [node for node in nodes if node["kind"] == "block"
                  and node["label"] in ("SH", "SS") and first["line"] < node["line"] < end]
    lines = source.split("\n")
    marks = []
    for boundary in boundaries:
        heading = lines[boundary["line"] - 1].partition(" ")[2]
        if heading not in HEADS:
            raise UnsupportedExtraction("unknown authored heading")
        body_label = "AFTERMARK" if not marks else "AFTERLATER"
        body = [node for node in nodes if node["kind"] == "text"
                and node["label"].startswith(body_label)
                and node["path"][:len(boundary["path"])] == boundary["path"]]
        if len(body) != 1:
            raise UnsupportedExtraction("cannot prove the following BODY word owner")
        marks.append(dict(line=boundary["line"], heading=heading, macro=boundary["label"],
                          bodyLine=body[0]["line"]))
    old_end = marks[0]["line"] if marks else end
    old_nodes = [node for node in nodes if first["line"] < node["line"] < old_end
                 and node["path"][:len(first["path"])] == first["path"] and not node["noPrint"]]
    words = [node for node in old_nodes if node["kind"] == "text" and "ALPHA" in node["label"]]
    if len(words) != 1:
        raise UnsupportedExtraction("no unique native ALPHA word in FIRST")
    word = words[0]
    same_coordinate = [node["label"] for node in old_nodes if node["kind"] == "text"
                       and node["line"] == word["line"] and node["lineStart"] and node["noFill"]]
    if ".PAY\n" in source and same_coordinate != [r"\&\c", word["label"]]:
        raise UnsupportedExtraction("macro expansion lacks two no-fill line events at one coordinate")
    tails = [node for node in old_nodes if node["kind"] == "text"
             and node["line"] > word["line"] and POST_LINK_MOTION.fullmatch(node["label"])]
    return dict(first=first["line"], end=end, marks=marks, word=word["label"],
                wordLine=word["line"], noFill=word["noFill"], sameCoordinate=same_coordinate,
                linkMotion=any(lines[node["line"] - 1] == ".UE " + node["label"]
                               or lines[node["line"] - 1] == ".ME " + node["label"]
                               for node in tails),
                authoredBr=[i for i, line in enumerate(lines, 1) if line == ".br"],
                nativeBr=[node["line"] for node in old_nodes if node["label"] == "br"],
                nativeSp=[node["line"] for node in old_nodes if node["label"] == "sp"],
                nativeFi=[node["line"] for node in old_nodes if node["label"] == "fi"])


def native_interval(raw, *, eof):
    rows = [device_row(row)[0] for row in raw.split("\n")[:-1]]
    if not eof:
        return between_sections([row.lstrip(" ") for row in rows])
    # CVS term_ascii prints one blank furniture line before the known
    # fixed document footer. No stripping of the actual preceding rows.
    if rows.count("FIRST") != 1 or len(rows) < 2 or rows[-2] or not KNOWN_FOOTER.fullmatch(rows[-1]):
        raise UnsupportedExtraction("cannot delimit EOF footer")
    return [row.lstrip(" ") for row in rows[rows.index("FIRST") + 1:-2]]


def reader_interval(plain, *, eof):
    rows = [row.lstrip(" \t") for row in plain.split("\n")]
    if not eof:
        return between_sections(rows)
    if rows.count("FIRST") != 1:
        raise UnsupportedExtraction("cannot delimit product EOF")
    return rows[rows.index("FIRST") + 1:]


def native_structural_rows(rows):
    expected = []
    head_indices = []
    split = []
    for row in rows:
        if HEAD_ROW.fullmatch(row):
            head_indices.append(len(expected))
            expected.append(row)
        elif ((match := re.search(r"(NEXTX?|EXT|LATER)$", row)) is not None
              and (old := row[:match.start()].rstrip(" "))
              and (OLD_ROW.fullmatch(old) or old.startswith("AFTERMARK"))):
            heading = match[0]
            head_indices.append(len(expected) + 1)
            expected.extend((old, heading))
            split.append((old, heading))
        else:
            if not OLD_ROW.fullmatch(row) and row not in ("AFTERMARK", "AFTERLATER"):
                raise UnsupportedExtraction(f"unmapped native section padding row: {row!r}")
            expected.append(row)
    return expected, head_indices, split


def reading_key(row):
    # This axis compares words and line positions, not scalar preservation.
    # Preserve every NBSP; only ASCII columns and TAB are presentation
    # positions here, checked independently when a precise witness exists.
    return re.sub("[ \t]+", " ", row.strip(" \t"))


def motion_plan(evidence, expected, heads, source):
    """An independently proven old literal SP contribution.

    term_word() writes ASCII_NBRSP for positive HORIZ (term.c:678-700);
    term_field() only emits it as advance before a later printable cell.
    Old readings keep one SP per advance request, without changing the
    number of completed native blank rows.
    """
    if not evidence["noFill"]:
        return None
    first = evidence["marks"][0] if evidence["marks"] else None
    if evidence["linkMotion"] and first and first["heading"] == "NEXT" and heads:
        # The pinned tree has a no-fill motion text node *after* the
        # source UR/MT body. The terminal only prints the closing URI
        # before the heading. The old reading retains exactly one SP
        # in its own source row, outside the clickable label.
        prior = expected[:heads[0]]
        target = next((line.partition(" ")[2] for line in source.split("\n")
                       if line.startswith((".UR ", ".MT "))), None)
        if (target and prior[:1] == ["ALPHA"] and prior[-1:] == [f"<{target}>"]
                and not any(prior[1:-1])):
            expected.insert(heads[0], "")
            return " ", prior, "shared-motion-row"
    match = PURE_MOTION.fullmatch(evidence["word"])
    if not match:
        return None
    if first and first["heading"] not in ("NEXT", r"\&", r"\z", r"NEXT\p WORD"):
        # A HEAD marker can split or reject the motion-containing pass.
        # Do not assume that the selected reading placeholder survives.
        return None
    blanks = match[1].count(r"\p\&")
    pad = " " * match[2].count(r"\h")
    first_body = next((i for i, row in enumerate(expected) if row == "AFTERMARK"), len(expected))
    boundary = heads[0] if heads else first_body
    if expected[:1] != ["ALPHA"] or any(expected[1:boundary]):
        return None
    if match[3]:
        # The exact covered-X control prints NEXT (not XNEXT) and no old
        # X. The positive advance and the source SP before the erased
        # glyph survive in the old word. Do not infer this for an X
        # that is visible in native or a HEAD rejected by term_fill.
        if not first or first["heading"] != "NEXT" or not heads:
            return None
        pad += " "
    prior = ["ALPHA", *([""] * blanks)]
    real_flush = (not first or first["macro"] == "SH" or ".PD 1\n" in source
                  or evidence["nativeBr"] or evidence["nativeSp"] or evidence["nativeFi"])
    if boundary == blanks + 1 and first and heads and not real_flush:
        # Native HEAD shares the motion's row. IR separates the retained
        # old owner, so this *one* SP-only old row is a known projection.
        expected.insert(boundary, "")
        return pad, prior, "shared-motion-row"
    if boundary >= blanks + 2:
        return pad, prior, "completed-motion-row"
    return None


def wrapped_label(word):
    # These wrappers cover the same old source line, including its hard
    # breaks. A font reset in that word or a separate UE/ME tail does not
    # wrap subsequent reading padding.
    word = word.removeprefix(r"\fI").removeprefix(r"\fB").partition(r"\fR")[0]
    match = WRAPPED_OLD_WORD.fullmatch(word)
    if not match:
        raise UnsupportedExtraction("unknown styled or linked old word")
    tail = match[2]
    if tail is None:
        return "ALPHA"
    padding = "\u00a0" if tail == r"\ " else "  BETA" if tail.endswith(" BETA") else " "
    return "ALPHA" + "\n" * (match[1].count(r"\p\&") + 1) + padding


def wrapper_axes(source, evidence, old, new):
    nodes = [node for block in old["blocks"] for node in block.get("children", ())]
    axes = {}
    for token, kind in ((".B ", "strong"), (r"\fB", "strong"),
                        (".I ", "emphasis"), (r"\fI", "emphasis")):
        if any(line.startswith(token) for line in source.split("\n")):
            value = "".join(inline_value(node["children"]) for node in inline_owners(nodes, kind))
            axes["old-font"] = exact(wrapped_label(evidence["word"]), value)
    for token, kind, key in ((".UR ", "external", "uri"), (".MT ", "email", "address")):
        target = next((line.removeprefix(token) for line in source.split("\n")
                       if line.startswith(token)), None)
        if target:
            links = list(inline_owners(nodes, "link"))
            axes["old-link-target"] = exact([dict(kind=kind, **{key: target})],
                                            [node["target"] for node in links])
            axes["old-link-label"] = exact(
                [wrapped_label(evidence["word"])], [inline_value(node["children"]) for node in links])
            axes["following-link"] = exact([], list(inline_owners(new["heading"]["content"], "link")))
    return axes


def body_axes(evidence, by_line, expected, old, roots):
    axes = {}
    marks = evidence["marks"]
    for index, mark in enumerate(marks):
        next_section = by_line[mark["line"]]
        suffix = "" if index == 0 else f"-{index + 1}"
        native_title = "LATER" if index else "NEXT"
        choices = ("LATER",) if index else ("NEXT", "EXT", "NEXTX", "WORD")
        native_heads = [row for row in expected if row in choices]
        if native_title == "LATER" and not native_heads:
            raise UnsupportedExtraction("second heading has no proven printed owner")
        axes["heading" + suffix] = exact(
            native_heads, inline_value(next_section["heading"]["content"]).strip(" \n").splitlines())
        axes["identity" + suffix] = exact(HEADS[mark["heading"]], next_section["id"])
        siblings = old["children"] if mark["macro"] == "SS" else roots
        axes["section-nesting" + suffix] = exact(True, next_section in siblings)
        label = "AFTERMARK" if not index else "AFTERLATER"
        rows = projected_rows(next_section["blocks"])
        axes["body" + suffix] = exact([label], [reading_key(row) for row in rows if reading_key(row)])
        axes["body-source" + suffix] = payload_source_axis(
            next_section["blocks"], mark["bodyLine"])
    return axes


def source_padding_axes(source, evidence, expected_old, old_rows):
    axes = {}
    word = source.split("\n")[evidence["wordLine"] - 1]
    old_text = "\n".join(old_rows)
    # Native UTF-8 writes these as NBSP, including when it shares the
    # head device row. Preserve scalar identity, not visual whitespace.
    if any(token in word for token in ("\u00a0", r"\ ", r"\~", r"\0")):
        axes["author-nbsp"] = exact(
            sum(row.count("\u00a0") for row in expected_old), old_text.count("\u00a0"))
    # No-fill preserves a source tab before accepted following text.
    # The native renderer uses columns for it, not a TAB byte.
    if evidence["noFill"] and "\t" in word and any("BETA" in row for row in expected_old):
        axes["author-tab"] = exact(1, old_text.count("\t"))
    elif ("\t" in word and re.search(r"\\p\\&\t (?:\\p\\& )?BETA$", word)
          and any("BETA" in row for row in expected_old)):
        # In filled text this TAB is deferred in an empty pass, so
        # term_field() never prints its advance before the row closes.
        axes["filled-deferred-tab"] = exact(0, old_text.count("\t"))
    first = evidence["marks"][0] if evidence["marks"] else None
    if first and first["macro"] == "SS" and ".PD 0\n" in source:
        if word == operand(0, " ", "BETA"):
            axes["author-ascii-space"] = exact("ALPHA  BETA", old_rows[0] if old_rows else None)
        if evidence["noFill"] and word == operand(0, "") + " ":
            axes["author-ascii-space"] = exact("ALPHA ", old_rows[0] if old_rows else None)
    return axes


def check_section_padding(source, tree, utf8, observation):
    try:
        evidence = source_witness(source, tree)
        eof = ".SH ENDTEST\n" not in source
        if utf8["code"] != 0 or not utf8["stdout"]["utf8"]:
            raise UnsupportedExtraction("no certified native UTF-8 output")
        raw = b64decode(utf8["stdout"]["base64"], validate=True).decode()
        native = native_interval(raw, eof=eof)
        expected, heads, split = native_structural_rows(native)
    except UnsupportedExtraction as error:
        return dict(state="review", reason=str(error))
    if observation is None:
        return dict(state="error", reason="missing direct observation", evidence=evidence)
    document = json.loads(observation["json"])["document"]
    by_line = {section.get("source", {}).get("line"): section
               for section in sections(document["sections"])}
    old = by_line.get(evidence["first"])
    if old is None or any(mark["line"] not in by_line for mark in evidence["marks"]):
        return dict(state="difference", reason="missing section owner", evidence=evidence)
    try:
        actual = reader_interval(observation["plain"], eof=eof)
        old_rows = projected_rows(old["blocks"])
    except UnsupportedExtraction as error:
        return dict(state="difference", reason=str(error), evidence=evidence)
    motion = motion_plan(evidence, expected, heads, source)
    if motion and motion[-1] == "shared-motion-row":
        heads = [i + 1 for i in heads]
    axes = {
        "reader-rows": exact(list(map(reading_key, expected)), list(map(reading_key, actual))),
        "transport": exact(True, all(observation[k] for k in
                                     ("validDocument", "jsonEquivalent", "ansiEquivalent"))),
    }
    boundary = heads[0] if heads else (
        expected.index("AFTERMARK") if evidence["marks"] else len(expected))
    expected_old = expected[:boundary]
    # A source PD1 boundary closes the prior owner, then adds a distinct
    # vertical request before HEAD. Its blank is not an old-tail receipt.
    if heads and ".PD 1\n" in source and expected_old[-1:] == [""]:
        expected_old = expected_old[:-1]
    if evidence["marks"] and evidence["marks"][0]["heading"] == r"\p NEXT" and not heads:
        if expected_old[-1:] == [""] and re.search(r"(?:BETA|\\&)\s*$", evidence["word"]):
            expected_old = expected_old[:-1]
    if (heads and evidence["marks"] and evidence["marks"][0]["heading"] == r"\p NEXT"
            and PURE_MOTION.fullmatch(evidence["word"]) and evidence["word"].endswith(r"\c")
            and expected_old[-1:] == [""]):
        # In the continued HORIZ control term_fill accepts a leading
        # HEAD pass before NEXT. Keep that row in HEAD, not in FIRST.
        expected_old = expected_old[:-1]
        title = by_line[evidence["marks"][0]["line"]]["heading"]["content"]
        axes["head-leading-row"] = exact("\nNEXT", inline_value(title))
    axes["old-content"] = exact(
        [reading_key(row) for row in expected_old if reading_key(row)],
        [reading_key(row) for row in old_rows if reading_key(row)])
    axes["old-interior"] = exact(interior(expected_old), interior(old_rows))
    if motion or not (evidence["marks"] and not heads and expected_old[-1:] == [""]):
        axes["old-tail-rows"] = exact(suffix_blanks(list(map(reading_key, expected_old))),
                                      suffix_blanks(list(map(reading_key, old_rows))))
    axes["old-source"] = payload_source_axis(old["blocks"], evidence["wordLine"])
    if motion:
        pad, prior, policy = motion
        evidence["readingMotion"] = dict(padding=pad, precedingRows=len(prior), policy=policy)
        old_literal = next((inline_value(block["children"]).split("\n") for block in old["blocks"]
                            if block["type"] == "preformatted"), [])
        axes["old-reading-padding"] = exact(
            pad, old_literal[len(prior)] if len(old_literal) > len(prior) else None)
        axes["old-literal-interior"] = exact(
            prior, old_literal[:len(prior)])
    axes.update(source_padding_axes(source, evidence, expected_old, old_rows))
    try:
        axes.update(body_axes(evidence, by_line, expected, old, document["sections"]))
        if evidence["marks"]:
            axes.update(wrapper_axes(source, evidence, old, by_line[evidence["marks"][0]["line"]]))
    except UnsupportedExtraction as error:
        return dict(state="review", reason=str(error), evidence=evidence, axes=axes,
                    native=native, actual=actual)
    if split:
        evidence["sharedDeviceRows"] = split
    state = ("difference" if any(axis[0] == "difference" for axis in axes.values()) else
             "allowed-reading-space-row" if motion and motion[-1] == "shared-motion-row" else
             "allowed-owner-split" if split else "asserted-axes-pass")
    return dict(state=state, evidence=evidence, axes=axes, native=native, actual=actual)


def interior(rows):
    keys = list(map(reading_key, rows))
    printed = [index for index, key in enumerate(keys) if key]
    return keys[printed[0]:printed[-1] + 1] if printed else []
