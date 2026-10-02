"""Immutable source and profile bindings of the live Rust consumer bridge."""

import hashlib
import itertools
import json
from pathlib import Path
import unittest

from scripts.roff.fixtures.generate_roff_execution_fixtures import reading_rows


FIXTURE = (Path(__file__).resolve().parents[3]
           / "crates/mant-engine/tests/roff_lowering/acceptance_axes/consumer_cases.json")
FIXTURE_ROOT = FIXTURE.parent.parent


class ConsumerFixtureContractTests(unittest.TestCase):
    def test_declaration_name_limits_bind_actual_head_operands_not_native_stack_limits(self):
        fixture = json.loads((FIXTURE_ROOT / "declaration_names/limits.json").read_text())
        self.assertEqual(fixture["header"]["count"], 3)
        self.assertEqual(fixture["header"]["limit"], 256)
        self.assertFalse(fixture["header"]["expectationsFromProduct"])
        self.assertEqual(fixture["header"]["oracleSha256"],
                         "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6")
        self.assertEqual(len(fixture["cases"]), 3)
        self.assertEqual({case["declaredCount"] for case in fixture["cases"]}, {255, 256, 257})
        self.assertEqual(len({case["id"] for case in fixture["cases"]}), 3)
        for case in fixture["cases"]:
            with self.subTest(case=case["id"]):
                count = case["declaredCount"]
                self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                                 case["sourceSha256"])
                self.assertIn("\n.It Xo\n", case["source"])
                self.assertIn("\n.Xc\nBodyWord\n", case["source"])
                words = [line.removeprefix(".Fl ") for line in case["source"].splitlines()
                         if line.startswith(".Fl ")]
                self.assertEqual(words, [f"flag{index}" for index in range(count)])
                self.assertEqual(case["nativeHead"], " ".join("-" + word for word in words))
                self.assertEqual(case["observedAst"]["typedFlCount"], count)
                self.assertEqual(case["observedAst"]["typedFlagsInOrder"], words)
                self.assertTrue(case["observedAst"]["headViaExplicitXo"])
                self.assertEqual(case["expectedNames"],
                                 ["-" + word for word in words] if count <= 256 else [])
                self.assertEqual(case["nameLimit"], count > 256)
                self.assertEqual(set(case["profiles"]), {"ascii", "utf8", "html", "tree", "lint"})
                for profile in case["profiles"].values():
                    self.assertIs(type(profile["code"]), int)
                    self.assertEqual(profile["code"], 0)
                    self.assertRegex(profile["stdoutSha256"], r"^[0-9a-f]{64}$")
                    self.assertRegex(profile["stderrSha256"], r"^[0-9a-f]{64}$")

    def test_declaration_names_keep_exact_sources_and_native_profile_bindings(self):
        fixture = json.loads((FIXTURE_ROOT / "declaration_names/cases.json").read_text())
        self.assertEqual(fixture["header"]["count"], 102)
        self.assertFalse(fixture["header"]["expectationsFromProduct"])
        self.assertEqual(fixture["header"]["oracleSha256"],
                         "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6")
        cases = fixture["cases"]
        self.assertEqual(len(cases), 102)
        self.assertEqual(len({case["id"] for case in cases}), 102)
        self.assertEqual(len({case["source"] for case in cases}), 102)
        self.assertEqual(sum(case["profiles"]["lint"]["code"] == 0 for case in cases), 79)
        self.assertEqual(sum(case["ownerProof"]["kind"] == "native-definition" for case in cases), 88)
        self.assertEqual(sum(case["ownerProof"]["kind"] == "literal-relative-body" for case in cases), 5)
        self.assertEqual(sum(case["ownerProof"]["kind"] == "no-owner" for case in cases), 9)
        for case in cases:
            with self.subTest(case=case["id"]):
                self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                                 case["sourceSha256"])
                self.assertEqual(set(case["profiles"]), {"ascii", "utf8", "html", "tree", "lint"})
                for name, profile in case["profiles"].items():
                    self.assertIs(type(profile["code"]), int)
                    self.assertIn(profile["code"], {0, 2} if name == "lint" else {0})
                    self.assertRegex(profile["stdoutSha256"], r"^[0-9a-f]{64}$")
                    self.assertRegex(profile["stderrSha256"], r"^[0-9a-f]{64}$")
                self.assertEqual(len(case["names"]), len(set(case["names"])))
                self.assertFalse(set(case["names"]) & set(case["excluded"]))
                proof = case["ownerProof"]
                if proof["kind"] == "native-definition":
                    self.assertIn(proof["headMacro"], {"TP", "IP", "It"})
                    self.assertRegex(case["source"], r"(?m)^\." + proof["headMacro"] + r"(?:$|\s)")
                elif proof["kind"] == "literal-relative-body":
                    self.assertIn(proof["headMacro"], {"B", "MR", "TEXT"})
                    if proof["headMacro"] == "TEXT":
                        self.assertIn("\\fB", case["source"])
                    else:
                        self.assertIn("." + proof["headMacro"] + " ", case["source"])
                    self.assertRegex(case["source"], r"(?m)^\.RS(?:$|\s)")
                else:
                    self.assertEqual(proof, {"kind": "no-owner", "headMacro": None})
                    self.assertEqual(case["kind"], "none")

    def test_hanging_owner_published_fixture_mirrors_exact_engine_resource(self):
        # The two published members need package-local resources. This mirror
        # is byte-identical, not a separately editable product-derived gold.
        engine = FIXTURE_ROOT / "hanging_owners/cases.json"
        repository = FIXTURE.parents[5]
        ui = repository / "crates/mant-ui/src/document/tests/hanging_owners/cases.json"
        self.assertEqual(ui.read_bytes(), engine.read_bytes())
        fixture = json.loads(engine.read_text())
        self.assertEqual(fixture["header"]["count"], 24)
        self.assertEqual(len(fixture["cases"]), 24)
        self.assertEqual(len({case["id"] for case in fixture["cases"]}), 24)
        for case in fixture["cases"]:
            self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                             case["sourceSha256"])

    def test_native_input_guard_keeps_exact_pristine_sources_and_profiles(self):
        fixture = json.loads((FIXTURE_ROOT / "native_input_limits/cases.json").read_text())
        self.assertEqual(fixture["header"]["count"], 2)
        self.assertEqual(fixture["header"]["referenceSha256"],
                         "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6")
        self.assertEqual(len(fixture["cases"]), 2)
        self.assertEqual([case["limited"] for case in fixture["cases"]], [True, False])
        for case in fixture["cases"]:
            self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                             case["sourceSha256"])
            self.assertEqual(set(case["profiles"]), {"ascii", "utf8", "html", "tree", "lint"})
            for profile in case["profiles"].values():
                self.assertEqual(profile["status"], 0)
                self.assertRegex(profile["stdoutSha256"], r"^[0-9a-f]{64}$")
                self.assertRegex(profile["stderrSha256"], r"^[0-9a-f]{64}$")

    def test_empty_word_published_fixture_mirrors_exact_engine_resource(self):
        # Each published crate owns its compile-time input. The mirror retains
        # the exact independently recorded 472-source matrix, without new gold.
        engine = FIXTURE_ROOT / "empty_word_columns/cases.json"
        ui = FIXTURE.parents[5] / "crates/mant-ui/src/document/tests/empty_word_columns/cases.json"
        self.assertEqual(ui.read_bytes(), engine.read_bytes())

    def test_empty_word_core_contains_every_declared_tuple(self):
        fixture = json.loads((FIXTURE_ROOT / "empty_word_columns/cases.json").read_text())
        atoms = {"zero-graph", "empty", "combining", "font-only",
                 "bare-zero-advance", "pending-zero-advance"}
        expected = set(itertools.product(
            ("filled", "no-fill"), (False, True), (1, 2, 3),
            ("No", "Em", "Sy", "Li", "Lk"), atoms))
        core = [case["metadata"] for case in fixture["cases"]
                if set(case["metadata"]) == {"mode", "start", "repeat", "carrier", "atom"}
                and case["metadata"]["atom"] in atoms]
        self.assertEqual(len(core), 360)
        actual = {(item["mode"], item["start"], item["repeat"],
                   item["carrier"], item["atom"]) for item in core}
        self.assertEqual(actual, expected)

    def test_empty_word_columns_keep_independent_sources_profiles_and_row_coverage(self):
        fixture = json.loads((FIXTURE_ROOT / "empty_word_columns/cases.json").read_text())
        header = fixture["header"]
        self.assertEqual((header["count"], header["unique_sources"]), (472, 472))
        self.assertEqual((header["asserted_row_count"], header["uncovered_row_count"]),
                         (460, 12))
        self.assertFalse(header["expectations_from_product"])
        self.assertEqual(header["oracle_sha256"],
                         "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6")
        self.assertEqual(len(fixture["cases"]), 472)
        self.assertEqual(len({case["id"] for case in fixture["cases"]}), 472)
        self.assertEqual(len({case["source"] for case in fixture["cases"]}), 472)
        for case in fixture["cases"]:
            with self.subTest(case=case["id"]):
                self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                                 case["source_sha256"])
                self.assertEqual(set(case["profile_sha256"]),
                                 {"ascii", "utf8", "html", "tree", "lint"})
                self.assertEqual(set(case["profile_status"]), set(case["profile_sha256"]))
                for name, status in case["profile_status"].items():
                    self.assertIs(type(status), int)
                    self.assertIn(status, {0, 1, 2} if name == "lint" else {0})
                    self.assertRegex(case["profile_sha256"][name], r"^[0-9a-f]{64}$")
                self.assertTrue(case["ordinary_body_owners"])
                for owner in case["ordinary_body_owners"]:
                    self.assertIsNone(owner["owner"])
                    self.assertIn(["Sh", "body", 7], owner["ancestors"])
                self.assertIn(case["row_observation"],
                              {"asserted", "uncovered-bold-combining-backspace"})
        self.assertEqual(sum(case["row_observation"] == "asserted"
                             for case in fixture["cases"]), 460)

    def test_reading_rows_retain_native_buffered_initial_separators(self):
        # The ten exact sources ran all five pristine profiles before this
        # assertion, byte-matching their original stored profile hashes.
        # term_word()573-589 writes these blanks into the source buffer;
        # they are not the common manual margin or a temporary device origin.
        identities = {f"formatter-{index:04d}"
                      for index in (92, 94, 95, 98, 99, 572, 574, 575, 578, 579)}
        path = FIXTURE_ROOT / "native_execution/fixtures/native_formatter_matrix.jsonl"
        selected = [case for line in path.read_text().splitlines()
                    if (case := json.loads(line))["id"] in identities]
        self.assertEqual(len(selected), 10)
        for case in selected:
            with self.subTest(case=case["id"]):
                rows, rule, paired = reading_rows(case, case["utf8_rows"],
                                                 case["terminal_heading"])
                self.assertEqual(rows, [row.removeprefix(" " * 5)
                                        for row in case["utf8_rows"]])
                self.assertEqual(rows, case["reading_utf8_rows"])
                self.assertEqual(rule, "only the five-column common manual margin is omitted")
                self.assertIsNone(paired)

    def test_every_selected_source_keeps_its_independent_five_profile_binding(self):
        # All exact sources ran pristine before the fixture and Rust behavior
        # assertions were written. This check never observes product output.
        fixture = json.loads(FIXTURE.read_text())
        self.assertEqual(fixture["oracle"]["sha256"],
                         "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6")
        self.assertEqual(len(fixture["cases"]), 34)
        names = set()
        for case in fixture["cases"]:
            with self.subTest(case=case["name"]):
                self.assertNotIn(case["name"], names)
                names.add(case["name"])
                self.assertEqual(hashlib.sha256(case["source"].encode()).hexdigest(),
                                 case["source_sha256"])
                self.assertEqual(set(case["profiles"]), {"ascii", "utf8", "html", "tree", "lint"})
                self.assertEqual(case["admission"], case["profiles"]["lint"]["status"])
                for name, profile in case["profiles"].items():
                    self.assertRegex(profile["stdout_sha256"], r"^[0-9a-f]{64}$")
                    self.assertRegex(profile["stderr_sha256"], r"^[0-9a-f]{64}$")
                    if name != "lint":
                        self.assertEqual(profile["status"], 0)
                self.assertEqual(case["rows"],
                                 [" ".join(cell for cell in row.split(" ") if cell)
                                  for row in case["native_rows"]])


if __name__ == "__main__":
    unittest.main()
