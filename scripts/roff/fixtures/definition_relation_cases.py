"""Exact source identities for definition row and word relation acceptance."""

HEADER = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n"
STYLES = ("tag", "hang", "inset", "ohang")
LABELS = {"short": "X", "long": "LongHeadWord", "unicode": "中e\\[u0301]", "fixed-space": "X\\~Y"}
LAST_ROWS = {
    "completed-graph-current-graph": ".It Xo AlphaWord\n.br\n.No BetaWord\n.Xc\n.No BodyWord\n",
    "completed-empty-current-graph": ".It Xo\n.br\n.No BetaWord\n.Xc\n.No BodyWord\n",
    "completed-graph-current-empty": '.It Xo AlphaWord\n.br\n.No ""\n.Xc\n.No BodyWord\n',
    "completed-graph-current-fixed": '.It Xo AlphaWord\n.br\n.No "\\0"\n.Xc\n.No BodyWord\n',
    "completed-graph-current-font": ".It Xo AlphaWord\n.br\n.No \\fB\n.Xc\n.No BodyWord\n",
    "completed-graph-current-zerowidth": ".It Xo AlphaWord\n.br\n.No \\&\n.Xc\n.No BodyWord\n",
    "completed-graph-current-styled": ".It Xo AlphaWord\n.br\n.Em BetaWord\n.Xc\n.No BodyWord\n",
    "completed-graph-current-link": ".It Xo AlphaWord\n.br\n.Lk x BetaWord\n.Xc\n.No BodyWord\n",
    "nofill-last-current-continued": ".nf\n.It Xo AlphaWord\n.No BetaWord\\c\n.Xc\n.No BodyWord\n.fi\n",
    "nofill-last-current-closed": ".nf\n.It Xo AlphaWord\n.No BetaWord\n.Xc\n.No BodyWord\n.fi\n",
    "head-fill-switch-last-continued": ".It Xo AlphaWord\n.nf\n.No BetaWord\\c\n.Xc\n.No BodyWord\n.fi\n",
    "head-fill-switch-last-closed": ".It Xo AlphaWord\n.nf\n.No BetaWord\n.Xc\n.No BodyWord\n.fi\n",
}


def sources():
    """Regenerate the full Cartesian identities independently of oracle gold."""
    result = {}
    for style in STYLES:
        width = " -width 4n" if style in ("tag", "hang") else ""
        opening = f".Bl -{style}{width}\n"
        for name, label in LABELS.items():
            for mode in ("filled", "literal", "continued"):
                prelude = ".nf\n" if mode != "filled" else ""
                continuation = "\\c" if mode == "continued" else ""
                result[f"layout-{style}-{name}-{mode}"] = (
                    HEADER + prelude + opening + f".It Xo\n.No {label}{continuation}\n.Xc\n.No BODY\n.El\n")
        for name, pattern in LAST_ROWS.items():
            prelude = ".nf\n" if pattern.startswith(".nf\n") else ""
            result[f"last-row-{style}-{name}"] = (
                HEADER + prelude + opening + pattern.removeprefix(prelude) + ".El\n")
    return result
