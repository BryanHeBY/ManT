"""Finite Markdown hard-row carriers and their exact roff source identities.

The core is the immutable 5 x 4 x 5 consumer cohort from the 2026-10-02
rule-closure review. Expectations are recorded by the pristine oracle, never
by a candidate Markdown renderer.
"""

import hashlib
import itertools

HEADER = (".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n"
          ".Nm test\n.Nd probe\n.Sh DESCRIPTION\n")
TAIL = ".Sh NEXT\n.No END\n"
HARD_LINES = (
    ("leading-one", r"\&\p AFTER"),
    ("leading-two", r"\&\p \&\p AFTER"),
    ("interior-empty", r"A\p \&\p AFTER"),
    ("trailing-one", r"AFTER\p"),
    ("interior-one", r"A\p B"),
)


def cases():
    """Yield cases with stable rule, reachability and consumer declarations."""
    for container, carrier, (topology, word) in itertools.product(
            ("paragraph", "tag", "hang", "column", "literal"),
            ("No", "Em", "Sy", "Lk"), HARD_LINES):
        body = f'.{carrier}' + (" https://ex.org" if carrier == "Lk" else "")
        body += f' "{word}"\n'
        if container in ("tag", "hang"):
            body = (f".Bl -{container} -width 8n\n.It Xo\n" + body
                    + ".Xc\n.No BodyWord\n.El\n")
        elif container == "column":
            body = ('.Bl -column "xxxxxxxx" "xxxx"\n.It Xo\n' + body
                    + ".Xc Ta RIGHT\n.El\n")
        elif container == "literal":
            body = ".Bd -literal -compact\n" + body + ".Ed\n"
        source = HEADER + body + TAIL
        yield {
            "id": f"M-core-{container}-{carrier.lower()}-{topology}",
            "rule_id": "RC05",
            "family": "markdown-hard-rows",
            "source": source,
            "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
            "metadata": {
                "container": container, "carrier": carrier,
                "hardline": topology, "operand": word,
                "source_class": "legal",
                "expected_axes": ["content", "rows", "separators", "style",
                                  "identity", "source", "json", "markdown-reader"],
                "consumers": ["plain", "ansi", "native-markdown",
                              "portable-markdown", "query", "tui-buffer",
                              "resize", "copy"],
                "uncovered": (["fenced-markdown-glyph-style",
                               "fenced-markdown-active-links"]
                              if container in ("literal", "column") else []),
            },
        }


iter_cases = cases
