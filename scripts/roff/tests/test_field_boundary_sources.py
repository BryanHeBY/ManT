"""Complete native-boundary manuals retain exact source and notice identities."""

import gzip
import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "tests/fixtures/roff/real/field-boundaries"
EXPECTED = {
    "debian-chem.1.gz": (16109, "a44beb1caf7bbf32571a8e820977b19636359a7bf535c9fdba0758a8c6705c32", 4),
    "netbsd-scmdctl.1": (8995, "221797315f5081d0982b0b288edefafd51464d66c3017a977ca61f0de615a904", 2),
    "openbsd-mandoc.1": (61688, "d5f5e2a732035be13413d22e75cdebc13f50bb2c4eac444cfe42b486883ab643", 0),
    "openbsd-ksh.1": (135857, "b121bd3bc53876205a5c0f37666e6a5234d899ad0632fdd429fb5d3f598a3fbe", 1),
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


class FieldBoundarySourceTests(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((FIXTURES / "manifest.json").read_bytes())

    def test_original_complete_members_and_transport_hashes(self):
        self.assertEqual({case["file"] for case in self.manifest["cases"]}, set(EXPECTED))
        self.assertEqual(self.manifest["originalManifestSha256"],
                         "d28f392f8b45a2ee80772310ce9f20173c6326585d05073f875d01483c774187")
        for case in self.manifest["cases"]:
            with self.subTest(file=case["file"]):
                raw = (FIXTURES / case["file"]).read_bytes()
                source = gzip.decompress(raw) if case["file"].endswith(".gz") else raw
                size, source_hash, _ = EXPECTED[case["file"]]
                self.assertEqual(len(source), size)
                self.assertEqual(digest(source), source_hash)
                self.assertEqual(case["sourceSha256"], source_hash)
                self.assertEqual(case["sourceBytes"], size)
                self.assertEqual(case["fixtureSha256"], digest(raw))
                self.assertEqual(case["fixtureBytes"], len(raw))

    def test_authored_notices_keep_their_own_terms(self):
        for case in self.manifest["cases"]:
            raw = (FIXTURES / case["file"]).read_bytes()
            source = gzip.decompress(raw) if case["file"].endswith(".gz") else raw
            first, last = case["embeddedNoticeLines"]
            notice = b"".join(source.splitlines(keepends=True)[first - 1:last])
            self.assertEqual(digest(notice), case["embeddedNoticeSha256"])
            terms = (FIXTURES / case["licenseFile"]).read_bytes()
            self.assertEqual(digest(terms), case["licenseSha256"])
            if case["file"] == "openbsd-ksh.1":
                self.assertIn(b"Public Domain", terms)
                self.assertTrue(case["license"].startswith("Public Domain"))
            elif case["file"] == "debian-chem.1.gz":
                self.assertEqual(case["license"], "GPL-3.0-or-later")
                self.assertIn(b"GNU GENERAL PUBLIC LICENSE", terms)
            else:
                self.assertIn(b"Permission to use, copy, modify", terms)

    def test_profiles_preserve_existing_lint_and_auxiliary_statuses(self):
        for case in self.manifest["cases"]:
            self.assertEqual(set(case["oracleProfiles"]),
                             {"ascii", "utf8", "html", "tree", "lint"})
            for profile, receipt in case["oracleProfiles"].items():
                expected = EXPECTED[case["file"]][2] if profile == "lint" else 0
                self.assertEqual(receipt["exitCode"], expected)
                self.assertRegex(receipt["stdoutSha256"], r"^[0-9a-f]{64}$")
                self.assertRegex(receipt["stderrSha256"], r"^[0-9a-f]{64}$")
            for receipt in case["groffProfiles"].values():
                self.assertEqual(receipt["exitCode"], 0)
                self.assertEqual(receipt["status"], "AuxiliaryCapturedOnly")


if __name__ == "__main__":
    unittest.main()
