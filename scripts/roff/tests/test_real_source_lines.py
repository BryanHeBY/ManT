"""Bind the real filled-TAB regression to the original package and oracle."""

import gzip
import hashlib
import json
from pathlib import Path
import unittest

from scripts.roff.fixtures import reference_recipes


ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "tests/fixtures/roff/real/source-lines"
SHA256 = {
    "fixture": "68aba5897f250fb1d81479dce2a471bf2e9b0598b45c339abb7a82816b4f1a2c",
    "source": "7be33290c550f8abb224dd9832d1fd5777449cf74175ebb689a3792cec452d77",
    "license": "c5ffbfeaa501071ceeb97b7de2c0d703fdaa35de01c0fb6cbac1c28453a3e9fd",
    "archive": "2b2e7ac64b1d56c08a227c10bcab179605f2773f31db0a8c89f49f4e5b2f1292",
    "reference": "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e",
}


class RealSourceLineTests(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((FIXTURE / "manifest.json").read_bytes())
        self.case, = self.manifest["cases"]

    def test_original_gzip_source_and_license(self):
        self.assertEqual(self.manifest["schema"], "mant.real-source-fixtures/v1")
        self.assertEqual(self.case["file"], "xcb_composite_unredirect_window.3.gz")
        self.assertEqual(self.case["archive"], "libxcb-1.17.0-1-x86_64.pkg.tar.zst")
        self.assertEqual(self.case["archiveMember"], "usr/share/man/man3/" + self.case["file"])
        self.assertEqual(self.case["licenseArchiveMember"], "usr/share/licenses/libxcb/COPYING")
        self.assertEqual(self.case["archiveSha256"], SHA256["archive"])
        stored = (FIXTURE / self.case["file"]).read_bytes()
        source = gzip.decompress(stored)
        license_bytes = (FIXTURE / self.case["licenseFile"]).read_bytes()
        self.assertEqual(self.case["sourceBytes"], len(source))
        self.assertEqual(self.case["fixtureBytes"], len(stored))
        for kind, data in [("fixture", stored), ("source", source), ("license", license_bytes)]:
            with self.subTest(kind=kind):
                self.assertEqual(self.case[kind + "Sha256"], SHA256[kind])
                self.assertEqual(hashlib.sha256(data).hexdigest(), SHA256[kind])
        self.assertEqual(
            [index for index, line in enumerate(source.splitlines(), 1) if line.startswith(b"\t")],
            [18, 21, 24],
        )

    def test_native_receipts_distinguish_filled_tab_warnings_from_render_failure(self):
        self.assertEqual(self.manifest["referenceSha256"], SHA256["reference"])
        self.assertEqual(self.manifest["referenceRecipe"],
                         reference_recipes.descriptor(reference_recipes.DEFAULT))
        profiles = self.case["oracleProfiles"]
        self.assertEqual(set(profiles), set(reference_recipes.PROFILES))
        for profile, receipt in profiles.items():
            self.assertEqual(receipt["arguments"], reference_recipes.arguments(profile))
            self.assertEqual(receipt["exitCode"], 2 if profile == "lint" else 0)
            self.assertGreater(receipt["stdoutBytes"], 0)
            self.assertRegex(receipt["stdoutSha256"], r"^[0-9a-f]{64}$")
            self.assertEqual(receipt["stderrBytes"], 0)
        self.assertEqual(self.case["wideUtf8"]["arguments"],
                         reference_recipes.arguments("utf8", width=160))
        self.assertEqual(self.case["wideUtf8"]["exitCode"], 0)


if __name__ == "__main__":
    unittest.main()
