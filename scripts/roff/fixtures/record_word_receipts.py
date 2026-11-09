"""Freeze exact native words and accepted rows before lowering is tested."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import gzip
from itertools import product
import json
from pathlib import Path
import re

from scripts.roff.fixtures.reference_recipes import digest, environment, PROFILES
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
REFERENCE = ROOT / "target/mandoc-migration/reference/mandoc"
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/word_receipts/cases.json.gz"
MDOC = (".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n"
        ".Nd word audit\n.Sh DESCRIPTION\n")
MAN = '.TH CHECK 1 "2026-11-10"\n.SH NAME\ncheck \\- word audit\n.SH DESCRIPTION\n'
END = ".Sh ENDTEST\n.No FINISH\n"

PREFIXES = {
    "ordinary": "ALPHA",
    "bare-zero": r"ALPHA\z",
    "held-graph": r"ALPHA\zX",
    "held-space": r"ALPHA\zX ",
    "held-tab": "ALPHA\\zX\t",
    "word-break": r"ALPHA\zX\p",
    "breakpoint-held": r"ALPHA\p\:\zX",
}
FOLLOWERS = {
    "plain": ".No BETA\n",
    "quoted-space": '.No " BETA"\n',
    "quoted-tab": '.No "\tBETA"\n',
    "fixed-space": '.No "\\ BETA"\n',
    "nbsp": '.No "\\~BETA"\n',
    "source-space": " BETA\n",
}
TAILS = {
    "empty": '.No ""\n.br\n.No OMEGA\n',
    "break": ".br\n.No OMEGA\n",
    "space-zero": ".sp 0\n.No OMEGA\n",
    "eof": "",
    "ordinary": ".No OMEGA\n",
}


def specification(identifier, cohort, source, comparison="exact"):
    return dict(id=identifier, cohort=cohort, source=source, comparison=comparison)


def specifications():
    for prefix, count, following, tail in product(PREFIXES, range(3), FOLLOWERS, TAILS):
        body = (f'.No "{PREFIXES[prefix]}"\n' + '.No ""\n' * count
                + FOLLOWERS[following] + TAILS[tail])
        # Terminal TAB stops are device geometry. The Rust test checks the
        # parser's original TAB and the nonempty rows of these sources.
        comparison = "device-tab" if prefix == "held-tab" or following == "quoted-tab" else "exact"
        end = "" if tail == "eof" else END
        yield specification(f"W1/{prefix}/{count}/{following}/{tail}", "W1", MDOC + body + end,
                            comparison)

    controls = (r"\& ", r"\:\& ", r"\fB ", r"\p ", r"\p\: ", r"\: ",
                r"\z \& ", r"\~ ", r" \& ", r"\&  ")
    for prefix, count in product(controls, (0, 1)):
        body = ('.No "ALPHA\\zX"\n' + '.No ""\n' * count + f'.No "{prefix}BETA"\n'
                + '.No ""\n.br\n.No OMEGA\n.Sh END\n.No ENDMARK\n')
        yield specification(f"W1-control/{prefix!r}/{count}", "W1-control", MDOC + body)

    for before, middle, tail in product(
            (r"ALPHA\zX\p\: ", r"ALPHA\zX\p\:\zY", r"ALPHA\zX\p\: \zY"),
            ("", '.No ""\n', ".No \\fB\n"),
            ("", '.No ""\n')):
        body = f'.No "{before}"\n{middle}.No " BETA"\n{tail}.br\n.No OMEGA\n'
        yield specification(f"W2/{before}/{middle!r}/{tail!r}", "W2", MDOC + body + END)

    for suffix, following in product(
            ("B", r"\zX", r"\zX ", "\\zX\t", r"\zX\zY", r"\zX\~", r"\&",
             r"\zX\p\:\zY", r"\zX\p\:\p REJECTED"),
            ("", '.No ""\n', ".No BETA\n", ".Aq BETA\n", ".br\n.No BETA\n")):
        body = f'.No "ALPHA\\p\\:{suffix}"\n{following}'
        comparison = "device-tab" if "\t" in suffix else "exact"
        yield specification(f"W3/{suffix}/{following!r}", "W3", MDOC + body, comparison)

    prefix = '.No "ALPHA\\zX"\n.No ""\n'
    tail = '.No ""\n.br\n.No OMEGA\n'
    for macro, label in product(("Em", "Li", "Lk"), ("BETA", "界e\u0301字")):
        target = " https://x.io" if macro == "Lk" else ""
        yield specification(f"W4/{macro}/{label}", "W4",
                            MDOC + prefix + f'.{macro}{target} " {label}"\n' + tail + END)
    for mode, begin, end in (
            ("no-fill", ".nf\n", ".fi\n"), ("keep", ".Bk -words\n", ".Ek\n"),
            ("spacing-off", ".Sm off\n", ".Sm on\n")):
        body = prefix + '.No " BETA"\n' + tail
        yield specification(f"W4/{mode}", "W4", MDOC + begin + body + end + END)
    for name, head, end in (("UR", "https://x.io", "UE"), ("MT", "u@x.io", "ME")):
        body = f'.{name} {head}\n.B "ALPHA\\zX"\n.B " BETA"\n.{end}\n.br\nOMEGA\n'
        yield specification(f"W4/{name}", "W4", MAN + body + ".SH ENDTEST\nFINISH\n")
    for macro in ("BR", "BI"):
        body = f'.{macro} "ALPHA\\zX" "" " BETA" ""\n.br\nOMEGA\n'
        yield specification(f"W4/{macro}", "W4", MAN + body + ".SH ENDTEST\nFINISH\n")
    for mode in ("tag", "hang", "inset", "diag", "column"):
        width = ' "20n" "10n"' if mode == "column" else (
            " -width 20n" if mode in ("tag", "hang") else "")
        item = ".It HEAD Ta\n" if mode == "column" else ".It HEAD\n"
        body = f".Bl -{mode}{width}\n{item}{prefix}.No \" BETA\"\n.No \"\"\n.El\n.No OMEGA\n"
        yield specification(f"W4/{mode}-body", "W4", MDOC + body + END, "device-field")
        restart = body.replace(prefix, '.No "ALPHA\\zX\\p\\: "\n.No ""\n', 1)
        yield specification(f"W2/{mode}-body", "W2", MDOC + restart + END, "device-field")
    include = '.No "ALPHA\\p\\:\\zX"\n.In stdio.h\n.No ""\n.br\n.No OMEGA\n'
    yield specification("W4/include", "W4", MDOC + include + END)

    for body, name in (
            ('.No "ALPHA\\zX"\n.Lk https://x.io " BETA"\n', "link-authored-space"),
            ('.No "ALPHA\\zX"\n.Lk https://x.io " \\zBETA"\n', "link-rearm"),
            ('.No "ALPHA\\zX"\n.de SEAM\n.No ""\n.No " BETA"\n..\n.SEAM\n', "same-coordinate"),
            ('.No "ALPHA\\zX" No "" No " BETA" No ""\n', "same-line"),
            ('.No "ALPHA\\zX"\n BETA\n', "raw-source-space")):
        yield specification(f"W5/{name}", "W5", MDOC + body + ".br\n.No OMEGA\n" + END)

    for label, source in (
            ("marker-no-suffix", '.No "ALPHA\\p"\n'),
            ("marker-space-no-suffix", '.No "ALPHA\\p "\n'),
            ("late-glyph", '.No "ALPHA\\p\\:\\zX"\n'),
            ("rejected", '.No "ALPHA\\p\\:\\p REJECTED"\n'),
            ("rejected-after-accepted", '.No "ALPHA\\p\\:X\\p\\:\\p REJECTED"\n')):
        yield specification(f"W6/{label}", "W6", MDOC + source)
    for count in range(4):
        body = '.Bd -literal\n.No ALPHA\n' + "\n" * count + ".Ed\n"
        yield specification(f"W6/literal-eof-{count}", "W6", MDOC + body)
        body = ".Bd -literal\n" + "\n" * count + ".No ALPHA\n.Ed\n"
        comparison = "reading-one-initial-row" if count == 1 else "exact"
        yield specification(f"W6/literal-initial-{count}", "W6", MDOC + body, comparison)
    for source in sorted(FIXTURE.parent.glob("*.1")):
        yield specification(f"consumer/{source.stem}", "consumer",
                            source.read_bytes().decode())


def terminal_text(row):
    # Simulate visible cells without normalizing any horizontal whitespace.
    cells = []
    column = 0
    for character in row:
        if character == "\b":
            column = max(column - 1, 0)
        else:
            if column == len(cells):
                cells.append(character)
            else:
                cells[column] = character
            column += 1
    return "".join(cells)


def native_rows(stdout, source):
    lines = stdout.decode().split("\n")
    start = next(index for index, row in enumerate(lines) if terminal_text(row) == "DESCRIPTION")
    next_section = ("ENDTEST" if "ENDTEST\n" in source
                    else "END" if ".Sh END\n" in source else None)
    if next_section:
        end = next(index for index in range(start + 1, len(lines))
                   if terminal_text(lines[index]) == next_section)
    else:
        end = len(lines) - 2
        assert lines[-1] == "" and lines[end].startswith("ManT ")
    body = lines[start + 1:end]
    # The section transition/footer always has exactly one furniture row.
    assert body[-1] == "", body
    margin = " " * 5
    rows = []
    for raw in body[:-1]:
        if raw.startswith(margin):
            raw = raw[len(margin):]
        # A leading device backspace targets the last generated margin
        # column. With the known margin already removed it has no effect.
        rows.append(terminal_text(raw))
    return rows


TREE = re.compile(r"^(.*) \((text|elem|block|head|body|tail)\).*? ([(*]*)(\d+):(\d+)(.*)$")


def tree_words(stdout, source):
    start = source.splitlines().index(".Sh DESCRIPTION" if source.startswith(".Dd")
                                     else ".SH DESCRIPTION") + 1
    stack = []
    result = []
    for row in stdout.decode().split("\n"):
        match = TREE.match(row)
        if not match:
            continue
        label, kind, flags, line, column, suffix = match.groups()
        indent = len(label) - len(label.lstrip(" "))
        if kind != "text":
            while stack and stack[-1][0] >= indent:
                stack.pop()
            stack.append((indent, label.lstrip(" "), kind))
            continue
        # tree.c adds two columns below a block and four below any other
        # owner. An authored leading blank cannot itself add a tree level.
        while stack and indent < child_indent(stack[-1]):
            stack.pop()
        if int(line) > start and stack:
            parent_indent, parent, parent_kind = stack[-1]
            offset = child_indent((parent_indent, parent, parent_kind))
            result.append([int(line), int(column), label[offset:], parent, parent_kind,
                           "*" in flags, "NOFILL" in suffix.split()])
    return result


def child_indent(parent):
    indent, _, kind = parent
    return indent + (2 if kind == "block" else 4)


def record(case):
    profiles = {}
    raw = {}
    for profile in PROFILES:
        args = ["-T" + profile, "-Ios=ManT"]
        if profile in ("ascii", "utf8"):
            args.append("-Owidth=160")
        run = run_reference(REFERENCE, args, input_bytes=case["source"].encode(),
                            env=environment())
        if profile != "lint" and run.returncode != 0:
            raise ValueError(f"{case['id']}/{profile}: {run.returncode}: {run.stderr!r}")
        profiles[profile] = [run.returncode, digest(run.stdout), digest(run.stderr)]
        raw[profile] = dict(stdout=run.stdout.decode(), stderr=run.stderr.decode())
    return (dict(case, source_sha256=digest(case["source"].encode()), profiles=profiles,
                 rows=native_rows(raw["utf8"]["stdout"].encode(), case["source"]),
                 tree_words=tree_words(raw["tree"]["stdout"].encode(), case["source"])), raw)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    registration = verified_reference(ROOT, REFERENCE)
    with ThreadPoolExecutor(max_workers=8) as executor:
        receipts = list(executor.map(record, specifications()))
    cases = [case for case, _ in receipts]
    fixture = dict(oracle_identity=registration["identity"],
                   oracle_sha256=digest(REFERENCE.read_bytes()), terminal_width=160, cases=cases)
    encoded = (json.dumps(fixture, ensure_ascii=False, indent=2) + "\n").encode()
    if args.write:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_bytes(gzip.compress(encoded, mtime=0))
    elif gzip.decompress(FIXTURE.read_bytes()) != encoded:
        raise ValueError("word receipt fixture differs from pristine profiles")
    if args.evidence:
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        with args.evidence.open("w") as output:
            for case, profiles in receipts:
                print(json.dumps(dict(case, raw=profiles), ensure_ascii=False), file=output)
    print(dict(Counter(case["cohort"] for case in cases)))


if __name__ == "__main__":
    main()
