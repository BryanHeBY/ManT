"""Complete real manuals retain their source and authored-license identities."""

import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "tests/fixtures/roff/real/scoped-layout"
EXPECTED = {
    "netbsd-printf.1": (13627, "42b7ffd3e1a67324c78e17deba0771c7c37cef8f083c31ec57927170dd3c8d70"),
    "openbsd-bpf.4": (28224, "be17bd1768ef2ef074d67b1d8a81ae350b8d58ce4125ddce456b45de5c853998"),
    "illumos-bhyve.8": (27747, "041ad1e505852a57be0e36e7105b5c80ea9767d457d92c607362519fe788c341"),
}


class ScopedLayoutSourceTests(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((FIXTURES / "manifest.json").read_bytes())

    def test_complete_sources_match_original_audit_not_generated_output(self):
        self.assertEqual(self.manifest["originalManifestSha256"],
                         "d28f392f8b45a2ee80772310ce9f20173c6326585d05073f875d01483c774187")
        cases = self.manifest["cases"]
        self.assertEqual({case["file"] for case in cases}, set(EXPECTED))
        for case in cases:
            with self.subTest(file=case["file"]):
                data = (FIXTURES / case["file"]).read_bytes()
                size, digest = EXPECTED[case["file"]]
                self.assertEqual(len(data), size)
                self.assertEqual(hashlib.sha256(data).hexdigest(), digest)
                self.assertEqual(case["sourceSha256"], digest)
                self.assertEqual(case["fixtureSha256"], digest)
                self.assertEqual(case["sourceBytes"], size)
                self.assertEqual(case["fixtureBytes"], size)
                self.assertNotIn(b"\r", data)

    def test_complete_notices_keep_distinct_license_conditions(self):
        notices = {}
        for case in self.manifest["cases"]:
            data = (FIXTURES / case["file"]).read_bytes()
            first, last = case["embeddedNoticeLines"]
            notice = b"".join(data.splitlines(keepends=True)[first - 1:last])
            self.assertEqual(hashlib.sha256(notice).hexdigest(), case["embeddedNoticeSha256"])
            license_bytes = (FIXTURES / case["licenseFile"]).read_bytes()
            self.assertEqual(hashlib.sha256(license_bytes).hexdigest(), case["licenseSha256"])
            notices[case["file"]] = license_bytes
            if case["file"] != "netbsd-printf.1":
                # The shared file strips only the roff comment prefix and its
                # one separating space; the complete authored notice remains.
                plain = []
                for line in notice.splitlines(keepends=True):
                    self.assertTrue(line.startswith(b'.\\"'))
                    content = line[3:]
                    plain.append(content[1:] if content.startswith(b" ") else content)
                self.assertEqual(license_bytes, b"".join(plain))
        self.assertIn(b"all advertising materials", notices["openbsd-bpf.4"])
        self.assertIn(b"Lawrence Berkeley Laboratory and its contributors.",
                      notices["openbsd-bpf.4"])
        self.assertIn(b"Copyright (c) 2013 Peter Grehan", notices["illumos-bhyve.8"])
        self.assertIn(b"Copyright 2026 Oxide Computer Company", notices["illumos-bhyve.8"])

    def test_captured_reference_profiles_do_not_hide_real_lint_diagnostics(self):
        lint_status = {"netbsd-printf.1": 2, "openbsd-bpf.4": 1, "illumos-bhyve.8": 1}
        for case in self.manifest["cases"]:
            self.assertEqual(set(case["oracleProfiles"]), {"ascii", "utf8", "html", "tree", "lint"})
            for name, profile in case["oracleProfiles"].items():
                expected = lint_status[case["file"]] if name == "lint" else 0
                self.assertEqual(profile["exitCode"], expected)
                self.assertRegex(profile["stdoutSha256"], r"^[0-9a-f]{64}$")
                self.assertRegex(profile["stderrSha256"], r"^[0-9a-f]{64}$")
                self.assertGreater(profile["stdoutBytes"], 0)


if __name__ == "__main__":
    unittest.main()
