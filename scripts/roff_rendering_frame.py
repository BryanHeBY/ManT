"""Conservative furniture masks for the original reduced behavior matrix.

Never remove an arbitrary first/last body line. Unrecognized native furniture
stays visible and makes framing partial. All removed raw rows are recorded.
"""
from __future__ import annotations

import platform
import re
import shlex

from roff_content_compare import visible_text


# Literal strings from the pinned upstream msec.in, not fuzzy volume matching.
VOLUMES = {"1": "General Commands Manual", "2": "System Calls Manual",
           "3": "Library Functions Manual", "3p": "Perl Library Manual",
           "4": "Device Drivers Manual", "5": "File Formats Manual",
           "6": "Games Manual", "7": "Miscellaneous Information Manual",
           "8": "System Manager's Manual", "9": "Kernel Developer's Manual"}


def plain(value: str) -> str:
    return " ".join(visible_text(value).split())


def prepare_frame(raw: str, source: str, *, reference: bool,
                  default_os: str | None = None) -> tuple[str, dict]:
    """Return original rows minus precisely identified title/header/footer.

    Supports explicit TH/Dt, known standard or explicit TH volumes and literal
    dates. It does not evaluate roff strings, macros, or computed metadata.
    """
    default_os = default_os or platform.system() + " " + platform.release()
    title = date = None
    os_name = ""
    dialect = None
    first_name = None
    section = volume = None
    for line in source.splitlines():
        if not line.startswith((".TH ", ".Dt ", ".Dd ", ".Os", ".Nm ")):
            continue
        try:
            words = shlex.split(line)
        except ValueError:
            continue
        if words[0] in (".TH", ".Dt") and len(words) >= 3:
            title = words[1] + "(" + words[2] + ")"
            section = words[2]
            volume = VOLUMES.get(section)
            dialect = "man" if words[0] == ".TH" else "mdoc"
            if dialect == "man":
                date = words[3] if len(words) > 3 else ""
                os_name = words[4] if len(words) > 4 else ""
                if len(words) > 5:
                    volume = words[5]
            elif len(words) > 3 and volume:
                volume += " (" + words[3].lower() + ")"
        elif words[0] == ".Dd":
            date = " ".join(words[1:])
        elif words[0] == ".Os":
            os_name = " ".join(words[1:]) or default_os
        elif (words[0] == ".Nm" and len(words) >= 2 and first_name is None
              and all(word in (",", ".", ";", ":") for word in words[2:])):
            first_name = words[1]
    lines = raw.splitlines(keepends=True)
    visible = [i for i, line in enumerate(lines) if plain(line)]
    report = {"schema": "mant.roff-source-frame/v1", "status": "partial",
              "title": title, "date": date, "os": os_name, "volume": volume,
              "default_os": default_os, "removedRows": [], "blankRowsTrimmed": 0,
              "reasons": []}
    if not title or not visible:
        report["reasons"].append("unsupported-or-empty-source-frame")
        return raw, report

    def remove(indices, kind):
        for index in indices:
            report["removedRows"].append({"row": index, "kind": kind, "raw": lines[index]})
            lines[index] = ""

    first = visible[0]
    if not reference:
        mant_titles = [title]
        if dialect == "mdoc" and first_name:
            mant_titles.append(first_name + "(" + section + ")")
        if plain(lines[first]) in mant_titles:
            remove([first], "mant-title")
            report["status"] = "covered"
        else:
            report["reasons"].append("unrecognized-mant-title")
        return "".join(lines), report

    # Header may wrap to three physical rows at width 20. Stop at its blank
    # separator; never search farther into the body for a plausible heading.
    head = []
    for index in range(first, min(len(lines), first + 5)):
        if not plain(lines[index]):
            break
        head.append(index)
    expected = title + " " + (volume or "")
    actual = " ".join(plain(lines[i]) for i in head)
    if volume is not None and actual in (expected, expected + " " + title):
        remove(head, "reference-header")
    else:
        report["reasons"].append("unrecognized-reference-header")

    # Footer can wrap OS and literal date independently. A complete exact
    # metadata sequence is required; a mere trailing title suffix is not enough.
    last = visible[-1]
    tail = []
    for index in range(last, max(-1, last - 6), -1):
        if not plain(lines[index]):
            break
        tail.append(index)
    tail.reverse()
    expected = " ".join(x for x in (os_name, date, title) if x)
    actual = " ".join(plain(lines[i]) for i in tail)
    if not report["reasons"] and date is not None and actual == expected and not set(tail).intersection(head):
        remove(tail, "reference-footer")
    else:
        report["reasons"].append("unrecognized-reference-footer")
    report["status"] = "partial" if report["reasons"] else "covered"
    return "".join(lines), report


def self_test() -> None:
    source = ".Dd September 9, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe ,\n"
    body, report = prepare_frame("probe(3)\n\nNAME\nprobe\n", source, reference=False)
    assert report["status"] == "covered" and "NAME\nprobe" in body
    assert plain("ABC\b") != "AB"
    source = '.TH PROBE 1 "2026-09-08"\n.SH TEST\nBODY\n'
    raw = "PROBE(1) General Commands Manual\n\nTEST\n BODY\n\n2026-09-08 PROBE(1)\n"
    body, report = prepare_frame(raw, source, reference=True)
    assert report["status"] == "covered" and "BODY" in body
    assert len(report["removedRows"]) == 2
    malformed = raw.replace("2026-09-08 PROBE(1)", "UNKNOWN BODY PROBE(1)")
    body, report = prepare_frame(malformed, source, reference=True)
    assert report["status"] == "partial" and "UNKNOWN BODY PROBE(1)" in body
    body, report = prepare_frame("BODY\nTAIL\n", source, reference=True)
    assert body == "BODY\nTAIL\n" and not report["removedRows"]
    source = ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\nBODY\n"
    raw = "PROBE(1)\nGeneral Commands\nManual\n\nTEST\n BODY\n\nLinux\nTESTKERNEL\n September 9, 2026\n PROBE(1)\n"
    body, report = prepare_frame(raw, source, reference=True, default_os="Linux TESTKERNEL")
    assert report["status"] == "covered" and "BODY" in body
    assert len(report["removedRows"]) == 7
    source = '.TH FOO 7 "2026-09-11"\n.SH TEST\nBODY\n'
    raw = "FOO(7) Miscellaneous Information Manual\n\nTEST\n BODY\n\n2026-09-11 FOO(7)\n"
    assert prepare_frame(raw, source, reference=True)[1]["status"] == "covered"
    source = '.TH FOO 8 "2026-09-11" "Source" "Custom Volume"\n.SH TEST\nBODY\n'
    raw = "FOO(8) Custom Volume FOO(8)\n\nTEST\n BODY\n\nSource 2026-09-11 FOO(8)\n"
    assert prepare_frame(raw, source, reference=True)[1]["status"] == "covered"


if __name__ == "__main__":
    self_test()
    print("roff rendering frame self-tests passed")
