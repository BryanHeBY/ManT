"""Bindings and admission for the permanent definition relation source matrix."""

import copy
import hashlib
import json
from pathlib import Path
import re
import unittest

from scripts.roff.fixtures.definition_relation_cases import sources

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/definition_relations/cases.json"
# Seal of the reviewed, original pristine observations, including both raw
# stream hashes and exact projected rows. This is snapshot integrity, not a
# substitute for a fresh certified reference run when changing expectations.
OBSERVATIONS_SHA256 = "1a7ed583736389326a31933f41bedca57ac436bed0622fcb9d3773f989d2970c"
FROZEN_REFERENCE = "/home/hby/dev/ManT/target/mandoc-migration/reference/mandoc"


def validate(fixture):
    if fixture.get("oracleIdentity") != "cvs-20260927T130954Z-linux-x86_64-gcc-16.2.1-reproducible-20260930" or fixture.get("oracleSha256") != "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6":
        raise ValueError("frozen producer identity changed")
    cases = fixture["cases"]
    expected_sources = sources()
    if len(cases) != 96 or {case["id"] for case in cases} != set(expected_sources):
        raise ValueError("definition relation matrix identity count changed")
    full = 0
    partial = []
    for case in cases:
        if case["source"] != expected_sources[case["id"]] or hashlib.sha256(case["source"].encode()).hexdigest() != case["sourceSha256"]:
            raise ValueError("definition source binding changed")
        if set(case["profiles"]) != {"ascii", "utf8", "html", "tree", "lint"}:
            raise ValueError("missing raw profile binding")
        for mode, record in case["profiles"].items():
            if any(not re.fullmatch(r"[0-9a-f]{64}", record[key]) for key in ["stdoutSha256", "stderrSha256"]):
                raise ValueError("malformed profile hash")
            expected_argv = [FROZEN_REFERENCE, "-T" + mode]
            if case["id"].startswith("layout-") and mode in ("ascii", "utf8"):
                expected_argv.append("-Owidth=78")
            if record["argv"] != expected_argv:
                raise ValueError("profile invocation changed")
            if type(record["status"]) is not int:
                raise ValueError("profile status is not an integer")
            if mode != "html" and record["status"] != 0:
                raise ValueError("terminal/tree/lint admission changed")
        if case["profiles"]["html"]["status"] == 0:
            full += 1
            if case["oracleScope"] != "five-profile":
                raise ValueError("successful profile scope changed")
        else:
            partial.append(case["id"])
            if case["profiles"]["html"]["status"] != -6 or not case["oracleScope"].startswith("terminal-only;"):
                raise ValueError("failed HTML profile falsely admitted")
    expected_partial = {f"last-row-{style}-head-fill-switch-last-{end}"
                        for style in ["tag", "hang", "inset", "ohang"]
                        for end in ["continued", "closed"]}
    if full != 88 or set(partial) != expected_partial:
        raise ValueError("native partial-profile identities changed")
    observations = json.dumps(fixture, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    if hashlib.sha256(observations).hexdigest() != OBSERVATIONS_SHA256:
        raise ValueError("reviewed pristine observation snapshot changed")


class DefinitionRelationFixtureTests(unittest.TestCase):
    def test_all_sources_and_raw_invocations_are_bound_without_erasing_failures(self):
        validate(json.loads(FIXTURE.read_text()))

    def test_mutated_source_missing_profile_and_false_success_are_rejected(self):
        fixture = json.loads(FIXTURE.read_text())
        for mutation in ["source", "rehash-source", "profile", "stdout", "stderr", "rows", "id", "binary", "argv", "status", "success", "count", "oracle"]:
            changed = copy.deepcopy(fixture)
            if mutation in ("source", "rehash-source"):
                changed["cases"][0]["source"] += ".No injected\n"
                if mutation == "rehash-source":
                    changed["cases"][0]["sourceSha256"] = hashlib.sha256(changed["cases"][0]["source"].encode()).hexdigest()
            elif mutation == "profile":
                del changed["cases"][0]["profiles"]["tree"]
            elif mutation in ("stdout", "stderr"):
                changed["cases"][0]["profiles"]["utf8"][mutation + "Sha256"] = "0" * 64
            elif mutation == "rows":
                changed["cases"][0]["nativeRows"] = ["BodyWord"]
            elif mutation == "id":
                changed["cases"][0]["id"] = "unreviewed-identity"
            elif mutation == "binary":
                changed["cases"][0]["profiles"]["utf8"]["argv"][0] = "/tmp/another-mandoc"
            elif mutation == "argv":
                changed["cases"][0]["profiles"]["utf8"]["argv"].append("-Owidth=120")
            elif mutation == "status":
                changed["cases"][0]["profiles"]["utf8"]["status"] = False
            elif mutation == "success":
                case = next(case for case in changed["cases"] if case["profiles"]["html"]["status"] != 0)
                case["profiles"]["html"]["status"] = 0
                case["oracleScope"] = "five-profile"
            elif mutation == "count":
                changed["cases"].pop()
            else:
                changed["oracleSha256"] = "0" * 64
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                validate(changed)


if __name__ == "__main__":
    unittest.main()
