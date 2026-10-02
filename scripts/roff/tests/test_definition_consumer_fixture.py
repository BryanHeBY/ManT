"""Bind exact native capacity/style and effective BODY boundary observations."""

import copy
import hashlib
import itertools
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/definition_consumers/cases.json"
# Seal of already captured pristine observations, never a replacement for a
# new certified run before changing a behavioral expectation.
OBSERVATIONS_SHA256 = "5423005b04bcf935423722710a898ece3583c7f3a1e5046d72871fc6fb81db80"
REFERENCE = "/home/hby/dev/ManT/target/mandoc-migration/reference/mandoc"
HEADER = ".Dd {date}\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n"


def sources():
    expected = {}
    carriers = ["No", "Sy", "Em", "Li", "Lk"]
    for width, head, body in itertools.product([3, 4, 5], carriers, carriers):
        expected[f"capacity-style-{width}-{head}-{body}"] = (
            HEADER.format(date="September 28, 2026")
            + f".Bl -hang -width 2n\n.It Xo\n.sp\n.{head} {'X' * width}\n.Xc\n.{body} BODY\n.El\n.Sh NEXT\n.No END\n"
        )
    requests = ["", ".br", ".Pp", ".sp", ".sp 0", ".sp 1", ".sp 2"]
    empties = ["", '.No ""', r".No \&", r".No \fB"]
    for style, (ri, request), (ei, empty) in itertools.product(
        ["hang", "tag", "inset", "ohang"], enumerate(requests), enumerate(empties)
    ):
        expected[f"body-{style}-{ri}-{ei}"] = (
            HEADER.format(date="October 2, 2026") + f".Bl -{style}"
            + (" -width 4n" if style in ["hang", "tag"] else "")
            + "\n.It HEAD\n" + (request + "\n" if request else "")
            + (empty + "\n" if empty else "")
            + ".No BODY\n.El\n.Sh NEXT\n.No END\n"
        )
    for width, glyphs, control in itertools.product([-2, -1, 0], [1, 2], ["sp", "br"]):
        label = str(width).replace("-", "minus") + "n"
        expected[f"zero-capacity-width{label}-chars{glyphs}-{control}"] = (
            HEADER.format(date="October 2, 2026")
            + f".Bl -hang -width {width}n\n.It Xo\n.{control}\n.No {'X' * glyphs}\n.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n"
        )
    return expected


def validate(fixture):
    if fixture["referenceSha256"] != "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6" or fixture["oracleIdentity"] != "cvs-20260927T130954Z-linux-x86_64-gcc-16.2.1-reproducible-20260930":
        raise ValueError("unregistered observation identity")
    expected = sources()
    cases = fixture["cases"]
    if len(cases) != 199 or {case["id"] for case in cases} != set(expected):
        raise ValueError("missing or duplicated matrix source")
    for case in cases:
        if case["source"] != expected[case["id"]] or hashlib.sha256(case["source"].encode()).hexdigest() != case["sourceSha256"]:
            raise ValueError("source binding changed")
        if set(case["profiles"]) != {"ascii", "utf8", "html", "tree", "lint"}:
            raise ValueError("missing profile")
        # These are actual retained invocations, including the bound source
        # file. Frozen paths describe the observation host, not a requirement
        # to find that file on a consumer's machine.
        if case["id"].startswith("zero-capacity-"):
            folder = "zero-capacity-review/" + case["id"].removeprefix("zero-capacity-")
        else:
            folder = "b-review/" + case["id"] if case["category"] == "capacity" else "body-prefix/" + case["id"].removeprefix("body-")
        input_path = "/home/hby/dev/ManT/target/audits/definition-consumer-repair-20261002/" + folder + "/input.1"
        for mode, record in case["profiles"].items():
            if type(record["status"]) is not int or record["status"] != 0 or record["argv"] != [REFERENCE, "-T" + mode, input_path]:
                raise ValueError("false profile admission")
            if any(not re.fullmatch(r"[0-9a-f]{64}", record[key]) for key in ["stdoutSha256", "stderrSha256"]):
                raise ValueError("unbound profile stream")
    observations = json.dumps(fixture, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    if hashlib.sha256(observations).hexdigest() != OBSERVATIONS_SHA256:
        raise ValueError("pristine observation snapshot changed")


class DefinitionConsumerFixtureTests(unittest.TestCase):
    def test_source_axes_profile_admission_and_exact_rows_are_bound(self):
        validate(json.loads(FIXTURE.read_text()))

    def test_source_profile_status_and_word_or_row_mutations_are_rejected(self):
        fixture = json.loads(FIXTURE.read_text())
        for axis in ["source", "source-rehashed", "row", "word", "hash", "profile", "status", "argv", "count"]:
            changed = copy.deepcopy(fixture)
            case = changed["cases"][0]
            if axis.startswith("source"):
                case["source"] += ".No injected\n"
                if axis == "source-rehashed":
                    case["sourceSha256"] = hashlib.sha256(case["source"].encode()).hexdigest()
            elif axis == "row":
                case["nativeRows"].pop(0)
            elif axis == "word":
                case["nativeRows"][1] = "     XXXBODY"
            elif axis == "hash":
                case["profiles"]["utf8"]["stdoutSha256"] = "0" * 64
            elif axis == "profile":
                del case["profiles"]["tree"]
            elif axis == "status":
                case["profiles"]["lint"]["status"] = False
            elif axis == "argv":
                case["profiles"]["utf8"]["argv"].append("-Owidth=120")
            else:
                changed["cases"].pop()
            with self.subTest(axis=axis), self.assertRaises(ValueError):
                validate(changed)


if __name__ == "__main__":
    unittest.main()
