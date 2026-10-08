"""Exercise installer notices without installing files or writing state."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]


class InstallerGuidanceTests(unittest.TestCase):
    def check_output(self, output, receipt):
        self.assertIn(f"receipt:    {receipt}", output)
        for text in ["Keep this receipt", "not automatically removed",
                     "mant --doctor", "only unused ManT-specific files",
                     "active/shared directories", "external tldr caches",
                     "system-wide directory variables",
                     "directory overrides are not rewritten", "manually-added PATH"]:
            self.assertIn(text, output)

    def test_both_install_and_current_paths_print_the_notice(self):
        unix = (ROOT / "scripts/install.sh").read_text()
        windows = (ROOT / "scripts/install.ps1").read_text()
        self.assertEqual(unix.count("  print_install_notes\n"), 2)
        self.assertEqual(windows.count("    Show-InstallationNotes\n"), 2)
        self.assertIn("print_install_notes\n  exit 0", unix)
        self.assertIn("Show-InstallationNotes\n    return", windows)

    @unittest.skipUnless(os.name == "posix", "Unix notice execution")
    def test_unix_notice_only_prints_without_creating_state(self):
        source = (ROOT / "scripts/install.sh").read_text()
        body = source.split("print_install_notes() {\n", 1)[1].split("\n}", 1)[0]
        with tempfile.TemporaryDirectory(prefix="mant guidance ") as directory:
            root = Path(directory)
            receipt = str(root / "state [case]/mant/install-receipt")
            result = subprocess.run(
                ["sh", "-c", f'receipt=$MANT_TEST_RECEIPT\n{body}\n'],
                env={**os.environ, "MANT_TEST_RECEIPT": receipt}, cwd=root,
                text=True, capture_output=True, timeout=10, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.check_output(result.stdout, receipt)
            self.assertEqual(list(root.iterdir()), [])

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell notice execution")
    def test_windows_notice_only_prints_without_creating_state(self):
        command = '''
$ErrorActionPreference = "Stop"
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($env:MANT_TEST_INSTALLER, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq "Show-InstallationNotes" }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$ReceiptPath = $env:MANT_TEST_RECEIPT
Show-InstallationNotes 6>&1 | Out-String
'''
        with tempfile.TemporaryDirectory(prefix="mant guidance ") as directory:
            root = Path(directory)
            receipt = str(root / "state [case]/mant/install-receipt.json")
            result = subprocess.run(
                ["pwsh", "-NoProfile", "-NonInteractive", "-Command", command],
                env={**os.environ, "MANT_TEST_RECEIPT": receipt,
                     "MANT_TEST_INSTALLER": str(ROOT / "scripts/install.ps1")},
                cwd=root, text=True, capture_output=True, timeout=30, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.check_output(result.stdout, receipt)
            self.assertEqual(list(root.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
