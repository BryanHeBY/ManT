"""Independent source-to-readback checks for the bounded Markdown lane.

Rows and owner paths are compared without trimming. The unsupported shapes
are explicit obligations, not passes inferred from two equal readbacks.
"""
import json

from .content_signature import AXES, STRUCTURAL_AXES, Review, document_signature
from .navigation_evidence import section_anchors


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
