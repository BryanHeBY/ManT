"""Record the incoming NOSPACE/Sm matrix from the pristine five-profile oracle.

Only the measured ORIGIN prefix is removed; authored SP/NBSP and every
interior physical row remain. No product executable supplies expectations.
"""

import argparse
import hashlib
import itertools
import json
from pathlib import Path

from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference
from scripts.roff.fixtures.reference_recipes import environment

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/word_spacing/cases.json"
PROFILES = ("ascii", "utf8", "html", "tree", "lint")
WORDS = {"empty": '""', "bold": r"\fB", "previous": r"\fP",
         "bare-z": r"\z", "z-c": r"\z\c"}
SUCCESSORS = {"text": "SECOND", "no": ".No SECOND",
              "link": ".Lk https://example.test SECOND"}
HEADER = (".Dd November 4, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n"
          ".Nd incoming word spacing\n.Sh DESCRIPTION\n")


def digest(raw):
    return hashlib.sha256(raw.encode() if isinstance(raw, str) else raw).hexdigest()


def source(word, initial="off", setting="on", successor="text", context="direct",
           join="", between=""):
    body = (f".No ORIGIN\n.br\n.Sm {initial}\n.No {word}{join}\n"
            f"{between}.Sm{(' ' + setting) if setting else ''}\n"
            f"{SUCCESSORS[successor]}\n.br\n.No TAIL\n")
    if context == "no-fill":
        body = ".nf\n" + body + ".fi\n"
    elif context == "display":
        body = ".Bd -filled\n" + body + ".Ed\n"
    elif context == "keep":
        body = ".Bk -words\n" + body + ".Ek\n"
    elif context.endswith("-head"):
        kind = context.removesuffix("-head")
        # HANG's term_newln flush can retain the same device row. Explicit
        # vertical requests bracket the measured HEAD region independently.
        body = body.replace(".br\n", ".sp 1\n") + ".sp 1\n"
        width = " -width 4n" if kind in ("tag", "hang") else ""
        body = f".Bl -{kind}{width}\n.It Xo\n{body}.Xc\n.No BodyWord\n.El\n"
    elif context.endswith("-body"):
        kind = context.removesuffix("-body")
        width = " -width 4n" if kind in ("tag", "hang") else ""
        body = f'.Bl -{kind}{width}\n.It ""\n.br\n{body}.El\n'
    return HEADER + body + ".Sh NEXT\nEND\n"


def specifications():
    for word, transition, successor in itertools.product(WORDS, ("on", ""), SUCCESSORS):
        yield {"id": f"core-{word}-{transition or 'toggle'}-{successor}",
               "cohort": "core", "source": source(WORDS[word], setting=transition,
                                                    successor=successor)}
    controls = [
        ("on-empty-on", dict(word='""', initial="on")),
        ("off-empty-ns-on", dict(word='""', join=" Ns")),
        ("off-c-on", dict(word=r"\c")),
        ("off-nbrzw-on", dict(word=r"\&")),
        ("off-visible-on", dict(word="FIRST")),
        ("off-empty-off", dict(word='""', setting="off")),
        ("off-empty-br-on", dict(word='""', between=".br\n")),
        ("off-empty-no-fill-on", dict(word='""', context="no-fill")),
        ("off-size-on", dict(word=r"\s12")),
        ("off-color-on", dict(word=r"\m[red]")),
        ("on-empty-off", dict(word='""', initial="on", setting="off")),
        ("on-visible-off", dict(word="FIRST", initial="on", setting="off")),
        ("on-empty-toggle-off", dict(word='""', initial="on", setting="")),
        ("on-nbrzw-off", dict(word=r"\&", initial="on", setting="off")),
        ("off-space-on", dict(word='" "')),
        ("off-consumed-glyph-on", dict(word=r"\zX")),
        ("on-visible-ns-on", dict(word="FIRST", initial="on", join=" Ns")),
        ("on-visible-c-on", dict(word=r"FIRST\c", initial="on")),
    ]
    for identifier, parameters in controls:
        yield {"id": "control-" + identifier, "cohort": "control",
               "source": source(**parameters)}
    for context, word, transition in itertools.product(
            ("no-fill", "display", "keep"), WORDS, ("on", "")):
        yield {"id": f"risk-{context}-{word}-{transition or 'toggle'}",
               "cohort": "risk", "source": source(WORDS[word], setting=transition,
                                                    context=context)}
    for index, context in enumerate(("tag-head", "hang-head", "inset-head",
                                     "tag-body", "hang-body", "inset-body", "diag-body")):
        for transition in ("on", ""):
            word = tuple(WORDS)[index % len(WORDS)]
            yield {"id": f"risk-{context}-{word}-{transition or 'toggle'}",
                   "cohort": "risk", "source": source(WORDS[word], setting=transition,
                                                        successor="no", context=context)}
    # Fn's selected pretty handler performs direct term_flushln, whereas a
    # subsequent br performs term_newln and explicitly selects NOSPACE.
    # Keep both directions without declaring either native boundary inert.
    flush_controls = [
        ("on-empty-on", '""', "on", "", "on", "", "text"),
        ("on-empty-br-on", '""', "on", "", "on", ".br\n", "text"),
        ("on-font-on", r"\fB", "on", "", "on", "", "text"),
        ("on-bare-z-on", r"\z", "on", "", "on", "", "text"),
        ("on-nbrzw-on", r"\&", "on", "", "on", "", "text"),
        ("on-cached-glyph-on", r"\zX", "on", "", "on", "", "text"),
        ("on-visible-on", "FIRST", "on", "", "on", "", "text"),
        ("off-empty-on", '""', "off", "", "on", "", "text"),
        ("off-empty-toggle-on", '""', "off", "", "", "", "text"),
        ("off-empty-off", '""', "off", "", "off", "", "text"),
        ("on-off-empty-on", '""', "on", ".Sm off\n", "on", "", "text"),
        ("on-off-empty-off", '""', "on", ".Sm off\n", "off", "", "text"),
        ("on-off-font-on", r"\fB", "on", ".Sm off\n", "on", "", "text"),
        ("off-on-empty-on", '""', "off", ".Sm on\n", "on", "", "text"),
        ("on-empty-off", '""', "on", "", "off", "", "text"),
        ("on-empty-on-link", '""', "on", "", "on", "", "link"),
    ]
    for label, word, initial, before, setting, request, successor in flush_controls:
        body = (f".No ORIGIN\n.br\n.Sm {initial}\n.Fn call\n{request}{before}"
                f".No {word}\n.Sm{(' ' + setting) if setting else ''}\n"
                f"{SUCCESSORS[successor]}\n.br\n.No TAIL\n.Sh NEXT\nEND\n")
        yield {"id": "flush-" + label, "cohort": "flush",
               "source": HEADER.replace(".Sh DESCRIPTION\n", ".Sh SYNOPSIS\n") + body}
        if label in ("on-empty-on", "on-empty-br-on"):
            yield {"id": "flush-fo-" + label, "cohort": "flush",
                   "source": HEADER.replace(".Sh DESCRIPTION\n", ".Sh SYNOPSIS\n")
                   + body.replace(".Fn call\n", ".Fo call\n.Fc\n")}


def projected_rows(raw):
    rows = []
    for row in raw.decode().splitlines():
        cells = []
        for character in row:
            if character == "\b":
                if cells:
                    cells.pop()
            else:
                cells.append(character)
        rows.append("".join(cells))
    start = next(i for i, row in enumerate(rows) if row.lstrip(" ") == "ORIGIN")
    end = next(i for i, row in enumerate(rows) if row.lstrip(" ") == "TAIL")
    origin = rows[start].index("ORIGIN")
    prefix = " " * origin
    return [row[len(prefix):] if row.startswith(prefix) else row for row in rows[start:end + 1]]


def record(reference, case, evidence=None):
    result = dict(case, source_sha256=digest(case["source"]), profiles={})
    for profile in PROFILES:
        arguments = ["-T" + profile] + (["-Owidth=78"] if profile in ("ascii", "utf8") else [])
        run = run_reference(reference, arguments, input_bytes=case["source"].encode(),
                            env=environment())
        if run.returncode != 0:
            raise ValueError(f"{case['id']}/{profile}: {run.returncode}: {run.stderr!r}")
        result["profiles"][profile] = dict(status=run.returncode,
                                         stdout_sha256=digest(run.stdout),
                                         stderr_sha256=digest(run.stderr),
                                         diagnostics=run.stderr.decode())
        if profile == "utf8":
            result["rows"] = projected_rows(run.stdout)
        if evidence:
            directory = evidence / case["id"]
            directory.mkdir(parents=True, exist_ok=True)
            (directory / "source.roff").write_text(case["source"])
            (directory / f"{profile}.stdout").write_bytes(run.stdout)
            (directory / f"{profile}.stderr").write_bytes(run.stderr)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--evidence", type=Path)
    arguments = parser.parse_args()
    reference = ROOT / "target/mandoc-migration/reference/mandoc"
    verified_reference(ROOT, reference)
    cases = [record(reference, case, arguments.evidence) for case in specifications()]
    result = dict(oracle_sha256=digest(reference.read_bytes()), core_count=30,
                  control_count=18, risk_count=44, flush_count=18,
                  distinct_source_count=len({case["source_sha256"] for case in cases}),
                  rule="term_word consumes incoming NOSPACE, then updates it from NONOSPACE; "
                       "termp_sm_pre clears NOSPACE only with live input cells when final mode is on.",
                  cases=cases)
    encoded = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if arguments.write:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(encoded)
    elif FIXTURE.read_text() != encoded:
        raise ValueError("word spacing fixture differs from the pristine profiles")
    print(f"verified {len(cases)} configurations / {result['distinct_source_count']} "
          "distinct word spacing sources in all five pristine profiles")


if __name__ == "__main__":
    main()
