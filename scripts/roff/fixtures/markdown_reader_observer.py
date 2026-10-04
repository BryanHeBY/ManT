"""Observe one source-bound hard-row cohort through its two JSON IRs.

This is not a Markdown parser or a roff executor. The reader already produced
its IR. Only the immutable RC05 core permits generated left origins, the
column fence separator and definition HEAD/BODY layout to be projected here.
Native/plain comparison remains independent of this consumer observation.
"""

from functools import lru_cache
import hashlib

from scripts.roff.fixtures import acceptance_comparison as comparison
from scripts.roff.fixtures.markdown_rule_cases import cases


AXES = ("content", "rows", "separators", "container", "body-ownership")


@lru_cache(maxsize=1)
def _core():
    return {case["source_sha256"]: case for case in cases()}


def _qualified(case):
    source = case.get("source")
    if not isinstance(source, str):
        return None
    digest = hashlib.sha256(source.encode()).hexdigest()
    if digest != case.get("source_sha256"):
        raise ValueError("Markdown consumer source binding changed")
    canonical = _core().get(digest)
    if canonical is None:
        return None
    for supplied, expected in (("container", "container"), ("context", "container"),
                               ("carrier", "carrier"), ("hardline", "hardline")):
        value = case.get("metadata", {}).get(supplied)
        if value is not None and value != canonical["metadata"][expected]:
            raise ValueError("Markdown consumer metadata does not match its exact source")
    return canonical


def _text(children):
    pieces = []
    for child in children:
        kind = child["type"]
        if kind in ("text", "code", "equation"):
            pieces.append(child["value"])
        elif kind == "line-break":
            if set(child) != {"type"}:
                raise ValueError("RC05 line-break must be a pure hard boundary")
            pieces.append("\n")
        elif kind == "anchor":
            continue
        elif kind in ("strong", "emphasis", "link"):
            pieces.append(_text(child["children"]))
        else:
            raise ValueError("unexpected RC05 inline kind: " + kind)
    return "".join(pieces)


def _blocks(bundle):
    sections = bundle["document"]["sections"]
    matches = [section for section in sections
               if _text(section["heading"]["content"]) == "DESCRIPTION"]
    if len(matches) != 1:
        raise ValueError("RC05 requires exactly one DESCRIPTION owner")
    return matches[0]["blocks"]


def _one(sequence):
    if len(sequence) != 1:
        raise ValueError("RC05 owner must retain exactly one output component")
    return sequence[0]


def _phrasing(block, kind):
    if block["type"] != kind:
        raise ValueError("RC05 output container changed")
    return block["children"]


def _term(term):
    if (not isinstance(term, dict) or "content" not in term
            or set(term) - {"content", "inlineLayout"}):
        raise ValueError("RC05 definition term must retain its content owner")
    return term["content"]


def _origins(value):
    # The exact 100-source core authors no leading SP/NBSP. These are only
    # resolved device origins; every LF and every internal scalar survives.
    return "\n".join(row.lstrip(" \u00a0") for row in value.split("\n"))


def _observe(core, original_bundle, reader_bundle):
    container = core["metadata"]["container"]
    original = _one(_blocks(original_bundle))
    reader = _one(_blocks(reader_bundle))
    ownership = True
    if container in ("tag", "hang"):
        if original["type"] != "definition-list" or reader["type"] != "list":
            raise ValueError("definition-to-bullet consumer topology changed")
        if reader["kind"] != {"kind": "bullet"}:
            raise ValueError("RC05 definition projection must remain a bullet")
        item = _one(original["items"])
        term = _text(_term(_one(item["terms"])))
        body = _text(_phrasing(_one(item["description"]), "paragraph"))
        ownership = ("BodyWord" not in term and body == "BodyWord"
                     and item.get("source", {}).get("line") == 9)
        relation = item.get("layout", {}).get("headBodyRelation", {"type": "separate"})
        if relation == {"type": "separate"}:
            separator = "\n"
        elif (isinstance(relation, dict)
              and set(relation) == {"type", "wordBoundary", "bodyAlignment"}
              and relation["type"] == "shared"
              and relation["wordBoundary"] in ("joined", "separated")
              and relation["bodyAlignment"] in ("after-term", "indented")):
            separator = "" if relation["wordBoundary"] == "joined" else " "
        else:
            raise ValueError("unknown RC05 HEAD/BODY relation")
        # Canonical Markdown keeps the accepted joined/separated word seam.
        # Preferred terminal columns do not add syntax-space content here.
        expected = term + separator + body
        actual = _text(_phrasing(_one(_one(reader["items"])["blocks"]), "paragraph"))
    elif container == "column":
        if original["type"] != "table":
            raise ValueError("RC05 column owner changed")
        row = _one(original["rows"])
        if len(row["cells"]) != 2:
            raise ValueError("RC05 column cell ownership changed")
        left = _text(_phrasing(_one(row["cells"][0]["blocks"]), "paragraph"))
        right = _text(_phrasing(_one(row["cells"][1]["blocks"]), "paragraph"))
        ownership = right == "RIGHT"
        # Keep the complete right-cell witness as well as every left-cell LF;
        # a missing/changed neighbor cannot be hidden by splitting at a pipe.
        expected = left + " | " + right
        actual = _text(_phrasing(reader, "preformatted"))
    else:
        kind = "preformatted" if container == "literal" else "paragraph"
        expected = _text(_phrasing(original, kind))
        actual = _text(_phrasing(reader, kind))
    return _origins(expected), _origins(actual), ownership


def markdown_reader_axes(case, original_bundle, reader_bundle):
    """Return source-qualified semantic consumer axes, without rendered bullets.

    Callers must first validate the pristine record/source/identity via the
    shared replay admission layer. Sources outside the 100-case core remain
    uncovered; matching core sources with missing/wrong IR are explicit failures.
    """
    try:
        core = _qualified(case)
    except (KeyError, TypeError, ValueError):
        return {**dict.fromkeys(AXES, "uncovered"), "source-binding": False}
    if core is None:
        return dict.fromkeys(AXES, "uncovered")
    try:
        expected, actual, ownership = _observe(core, original_bundle, reader_bundle)
    except (KeyError, TypeError, ValueError, AttributeError, IndexError):
        return dict.fromkeys(AXES, False)
    report = comparison.compare_axes({"status": "asserted", "rows": expected.split("\n")},
                                     {"status": "asserted", "rows": actual.split("\n")})
    return {axis: report[axis] for axis in ("content", "rows", "separators")} | {
        "container": True, "body-ownership": ownership,
    }
