"""Emit UTF-8 JSONL inputs for the publication review. Never derive expected values here."""
from hashlib import sha256
from itertools import product
import json

D = ".Dd November 7, 2026\n.Dt DEMO 1\n.Os\n.Sh NAME\n.Nm demo\n.Nd test\n.Sh "
M = '.TH DEMO 1 "November 7, 2026"\n.SH NAME\ndemo \\- test\n.SH '
U = "https://one.test"
R = r"printf(3) \%<>"
EQ = ".EQ\nx sup 2\n.EN"
headers = {"man": M, "mdoc": D}
ids = set()


def emit(family, axes, dialect, body, queries=(), section="DESCRIPTION"):
    key = family + "/" + "/".join(axes)
    assert key not in ids, key
    ids.add(key)
    source = headers[dialect] + section + "\n" + body + "\n"
    row = dict(id=key, family=family, axes=list(axes), dialect=dialect, section=section,
               source=source, sha256=sha256(source.encode("utf-8")).hexdigest(),
               queries=list(queries))
    print(json.dumps(row, ensure_ascii=True, sort_keys=True, separators=(",", ":")))


def link(kind, body):
    if kind == "UR":
        return ".UR " + U + "\n" + body + "\n.UE"
    if kind == "MT":
        return ".MT x@example.test\n" + body + "\n.ME"
    assert kind == "plain"
    return body


def items(body):
    return ".Bl -tag -width 4n\n" + body + "\n.El"


# N1a: classification and direct matches have independent gold.
heads = {"digit": ("Fl 1", "-1"), "flag": ("Fl f", "-f"),
         "command": ("Cm run", "run"), "env": ("Ev DEMO_HOME", "DEMO_HOME")}
tails = {"xr": "Xr printf 3", "ar": "Ar FILE", "lk": "Lk https://example.test FILE",
         "mt": "Mt y@example.test", "sy": "Sy FILE"}
for section, first, tail in product(("DESCRIPTION", "OPTIONS", "ENVIRONMENT"), heads, tails):
    head, query = heads[first]
    emit("N1a", (section, first, tail), "mdoc",
         items(".It " + head + " " + tails[tail] + "\nBODY"), (query,), section)

# N1b: empty owner, description provider, both, provider BODY, no link.
for place, separator in product(("empty", "provider", "both", "body", "none"),
                               ("adjacent", "if0", "if1", "owntext")):
    a = ".It Fl a" + (" Xr printf 3" if place in ("empty", "both") else "")
    b = ".It Fl b" + (" Xr printf 3" if place in ("provider", "both") else "")
    middle = {"adjacent": "", "if0": ".if 0 .Pp\n", "if1": ".if 1 .Pp\n",
              "owntext": "OWNBODY\n"}[separator]
    body = a + "\n" + middle + b + "\nSHAREDBODY"
    if place == "body":
        body += "\n.Xr printf 3"
    emit("N1b", (place, separator), "mdoc", items(body), ("-a", "-b"))

# N1c: HP and receiver pairing; the text classifier alone does not prove the pair.
hp = {"mr": ".B --one\n.MR printf 3", "tty": r"\X'tty: link https://one.test'\fB--one\fR\X'tty: link'",
      "ur": link("UR", ".B --one"), "none": ".B --one"}
for kind, receiver in product(hp, ("headless", "named", "intervening")):
    req = {"headless": ".IP", "named": ".IP LABEL", "intervening": ".PP\nOTHERPARA\n.IP"}[receiver]
    emit("N1c", (kind, receiver), "man", ".HP\n" + hp[kind] + "\n" + req + "\nSHAREDBODY",
         ("--one",), "OPTIONS")

# N2: split is an additional mdoc operand, not an invented UR/MT operand.
prefix = {"none": "", "zero": r"\&", "break": r"\:", "font": r"\fB", "nbsp": r"\~",
          "space": " ", "recovery": r"\[recovered]", "visible": "A"}
for wrap, pre, atom, width in product(("Lk", "UR", "MT"), ("first", "prev"), prefix, ("1n", "2n")):
    for seam in (("same", "split") if wrap == "Lk" else ("same",)):
        payload = prefix[atom] + ('" "' if seam == "split" else "") + r"\h'" + width + "'LABEL"
        body = ('.Lk ' + U + ' "' + payload + '"' if wrap == "Lk" else link(wrap, payload))
        if pre == "prev":
            body = (".No PRE\n" if wrap == "Lk" else "PRE\n") + body
        emit("N2", (wrap, pre, atom, width, seam), "mdoc" if wrap == "Lk" else "man", body + "\nAFTER")

# N3: external gap versus an internal literal row. No normalisation of leading blank TEXT.
gaps = {"one": ".sp", "two": ".sp 2", "zero": ".sp 0", "twice": ".sp\n.sp 2",
        "br": ".br", "none": ""}
for wrap, before, gap in product(("UR", "MT", "plain"), ("first", "blank", "real"), gaps):
    initial = {"first": "", "blank": "\n", "real": "REALLINE\n"}[before]
    inner = (gaps[gap] + "\n" if gaps[gap] else "") + "LASTMARK"
    body = ".nf\n" + initial + link(wrap, inner) + "\n.fi\nTAILMARK"
    emit("N3", (wrap, before, gap), "man", body)

# N4: changed/unchanged fill independently of conditional/actual break.
before_word = {"text": "FIRSTMARK", "bold": ".B FIRSTMARK", "zero": r"\&",
               "continue": r"FIRSTMARK\c"}
for wrap, mode, control, word in product(("UR", "MT", "plain"), ("filled", "nofill"),
                                        ("EE", "EX", "fi", "nf", "br"), before_word):
    inner = before_word[word] + "\n." + control + "\nLASTMARK"
    body = (".nf\n" if mode == "nofill" else "") + link(wrap, inner) + "\nTAILMARK"
    emit("N4", (wrap, mode, control, word), "man", body)

# E1/2: literal suffix vs typed source witness independent of traditional MR recognition.
suffixes = {"exact": r" \%<>", "raw": " <>", "none": "", "near": " <X>"}
for suffix, spelling, scope in product(suffixes, ("word", "BR", "IR"),
                                      ("plain", "UR", "MT", "outside", "nofill", "code")):
    tail = suffixes[suffix]
    inner = ("printf(3)" + tail if spelling == "word" else
             "." + spelling + ' printf "(3)' + tail + '"')
    if scope == "code":
        # Font-controlled text; combining a macro request with literal \f is a different grammar.
        if spelling != "word":
            continue
        body = r"\f(CR" + inner + r"\fP"
    elif scope == "nofill":
        body = ".nf\n" + inner + "\n.fi"
    elif scope == "outside":
        body = link("UR", "FIRST\n.PP\n" + inner)
    else:
        body = link(scope, inner)
    emit("E12", (suffix, spelling, scope), "man", body, ("printf",))
for key, dialect, body in (
    ("twice", "man", link("UR", R + r" and open(2) \%<>")),
    ("around", "man", R + "\n" + link("UR", R) + "\n" + R),
    ("lk", "mdoc", '.Lk ' + U + ' "' + R + '"'),
    ("tty", "man", r"\X'tty: link https://one.test'" + R + r"\X'tty: link'"),
):
    emit("E12extra", (key,), dialect, body, ("printf",))

# E3: include the last, sole, interior, config-only and genuinely out-of-scope equation.
for wrap, shape in product(("UR", "MT"), ("eq", "first-eq", "eq-last", "first-eq-last",
                                          "config", "config-word", "after", "closed")):
    inner = {"eq": EQ, "first-eq": "FIRST\n" + EQ, "eq-last": EQ + "\nLAST",
             "first-eq-last": "FIRST\n" + EQ + "\nLAST", "config": ".EQ\ndelim $$\n.EN",
             "config-word": ".EQ\ndelim $$\n.EN\nTXT", "after": "FIRST",
             "closed": "FIRST\n.nf\n" + EQ + "\n.fi"}[shape]
    body = link(wrap, inner) + ("\n" + EQ if shape == "after" else "") + "\nTAIL"
    emit("E3", (wrap, shape), "man", body, ("TAIL",))

# X: explicit, mandatory higher-order interactions.
for suffix, target in product(("raw", "exact"), ("head", "body")):
    token = R if suffix == "exact" else "printf(3) <>"
    head = "Fl a" + (' Lk ' + U + ' "' + token + '"' if target == "head" else "")
    body = ".It " + head + "\n.It Fl b\nSHAREDBODY" + ("\n" + token if target == "body" else "")
    emit("X-head-marker", (suffix, target), "mdoc", items(body), ("-a", "-b", "printf"))
for wrap, gap in product(("UR", "MT"), ("zero", "one", "two")):
    sp = {"zero": "", "one": ".sp\n", "two": ".sp 2\n"}[gap]
    first = sp + r"\&\h'2n'LABEL"
    emit("X-prefix-gap", (wrap, gap), "man", ".nf\n" + link(wrap, first) + "\n.fi\nTAIL")
for wrap, edge, tail in product(("UR", "MT"), ("EE", "fi", "nf", "PP"), ("last", "more")):
    inner = "FIRST\n." + edge + "\n" + EQ + ("\nLAST" if tail == "more" else "")
    emit("X-line-eqn", (wrap, edge, tail), "man", link(wrap, inner) + "\nTAIL", ("TAIL",))
