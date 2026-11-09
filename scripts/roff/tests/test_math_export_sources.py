"""Complete mathematical manuals keep their source, license and oracle bindings."""

import gzip
import hashlib
import json
from pathlib import Path
import unittest

from scripts.roff.fixtures import reference_recipes


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "tests/fixtures/roff/real/mathematical-export"
SOURCES = {
    "posix-float.h.0p": "962eb3e0f4a29eece96a0c28da6a818dc9b9a3ac575581b1797666addb49772e",
    "posix-stdint.h.0p": "62620ef7a8acb897256d4267bce5f2484b131034a88cc1f407d22178aac3d60e",
    "posix-drand48.3p": "6965cab009af539ad78c9ff66e51fb1bb242488c702c28793ef059f3c0fdbd2d",
    "openbsd-erf.3": "975ddb1cab2d67d7118dda0828539bcc1c445c3806ada4465ca2c6408c9ea278",
    "openbsd-lgamma.3": "8d696a81288381e0060b213bcf1f02d337b908f9ead0480ccc081656150f822d",
    "freebsd-stdc_bit_width.3": "7ba026709493d5e2f635f808a1d0d339f8a2a8fa8df9729b97f09c76d031bcac",
    "illumos-eqn.1": "26d43e92a68a756ec923d42652a6435c22a683c8a67c0bc54a4836d291292692",
    "xorg-XDrawArc.3.gz": "05f42581f61503352e688122cd53752823a7cc8626fbf48e4c401ca326fde5b0",
}
LINT = {
    "posix-float.h.0p": 3,
    "posix-stdint.h.0p": 2,
    "posix-drand48.3p": 2,
    "openbsd-erf.3": 0,
    "openbsd-lgamma.3": 0,
    "freebsd-stdc_bit_width.3": 1,
    "illumos-eqn.1": 2,
    "xorg-XDrawArc.3.gz": 2,
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


class MathematicalSourceTests(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((FIXTURES / "manifest.json").read_bytes())

    def test_complete_sources_and_transport_bytes(self):
        self.assertEqual(self.manifest["schema"], "mant.real-source-fixtures/v1")
        self.assertEqual({case["file"] for case in self.manifest["cases"]}, set(SOURCES))
        for case in self.manifest["cases"]:
            with self.subTest(file=case["file"]):
                stored = (FIXTURES / case["file"]).read_bytes()
                source = gzip.decompress(stored) if case["file"].endswith(".gz") else stored
                self.assertEqual(digest(source), SOURCES[case["file"]])
                self.assertEqual(case["sourceSha256"], digest(source))
                self.assertEqual(case["fixtureSha256"], digest(stored))
                self.assertEqual(case["sourceBytes"], len(source))
                self.assertEqual(case["fixtureBytes"], len(stored))
                self.assertTrue(case["originUrl"].startswith("https://"))
                self.assertTrue(case["selectionReason"])
                terms = (FIXTURES / case["licenseFile"]).read_bytes()
                self.assertEqual(digest(terms), case["licenseSha256"])
                self.assertGreater(len(terms), 500)
                if case["file"] == "xorg-XDrawArc.3.gz":
                    first, last = case["embeddedNoticeLines"]
                    notice = b"".join(source.splitlines(keepends=True)[first - 1:last])
                    self.assertEqual(digest(notice), case["embeddedNoticeSha256"])
                    plain = b"".join(line[4:] if line.startswith(b'.\\" ') else b"\n"
                                     for line in notice.splitlines(keepends=True))
                    self.assertEqual(plain, terms)
                    self.assertTrue(terms.endswith(b'It is provided "as is" without express or implied warranty.\n'))

    def test_native_profiles_record_the_real_diagnostics_and_recipe(self):
        self.assertEqual(self.manifest["referenceSha256"],
                         "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e")
        self.assertEqual(self.manifest["referenceRecipe"],
                         reference_recipes.descriptor(reference_recipes.DEFAULT))
        for case in self.manifest["cases"]:
            self.assertEqual(set(case["oracleProfiles"]), set(reference_recipes.PROFILES))
            for profile, receipt in case["oracleProfiles"].items():
                self.assertEqual(receipt["arguments"], reference_recipes.arguments(profile))
                self.assertEqual(receipt["exitCode"], LINT[case["file"]] if profile == "lint" else 0)
                self.assertRegex(receipt["stdoutSha256"], r"^[0-9a-f]{64}$")
                self.assertRegex(receipt["stderrSha256"], r"^[0-9a-f]{64}$")
                if profile != "lint":
                    self.assertGreater(receipt["stdoutBytes"], 0)


if __name__ == "__main__":
    unittest.main()
