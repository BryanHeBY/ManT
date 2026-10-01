"""Tool transport regressions; no native renderer or oracle substitution."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from scripts.roff.lib.roff_reference import module_command, reference_environment


class ToolEntrypointTests(unittest.TestCase):
    def check_relative_attestation(self, command, *, shadow=False):
        with tempfile.TemporaryDirectory(prefix="mant-tool-cwd-") as directory:
            if shadow:
                package = Path(directory, "scripts")
                package.mkdir()
                (package / "__init__.py").write_text("raise RuntimeError('wrong scripts package')")
            # An existing invalid record distinguishes caller-relative lookup
            # from accidentally looking up a missing file in the checkout.
            Path(directory, "attestation.json").write_text("invalid JSON")
            result = subprocess.run(
                [*command, "--attestation", "attestation.json",
                 "--binary", "oracle", "--archive", "archive.tar.gz"],
                cwd=directory, env=reference_environment(), capture_output=True,
                text=True, timeout=10, check=False,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertEqual(result.stdout, "")
            self.assertIn("invalid oracle attestation JSON", result.stderr)
            self.assertNotIn("ModuleNotFoundError", result.stderr)

    def test_owned_module_child_keeps_caller_relative_arguments(self):
        self.check_relative_attestation(
            module_command("scripts.roff.oracle.mandoc_oracle")
        )

    def test_owned_module_child_ignores_the_callers_scripts_package(self):
        self.check_relative_attestation(
            module_command("scripts.roff.oracle.mandoc_oracle"), shadow=True,
        )

    @unittest.skipUnless(os.name == "posix", "Bash entrypoint requires POSIX")
    def test_stable_oracle_wrapper_keeps_caller_relative_arguments(self):
        root = Path(__file__).resolve().parents[3]
        self.check_relative_attestation(["bash", str(root / "scripts/mandoc-oracle-preflight")])

    @unittest.skipUnless(os.name == "posix", "Bash entrypoint requires POSIX")
    def test_stable_oracle_wrapper_ignores_the_callers_scripts_package(self):
        root = Path(__file__).resolve().parents[3]
        self.check_relative_attestation(
            ["bash", str(root / "scripts/mandoc-oracle-preflight")], shadow=True,
        )

    @unittest.skipUnless(os.name == "posix", "Bash entrypoints require POSIX")
    def test_rebuild_and_fixture_wrappers_load_the_owned_command(self):
        root = Path(__file__).resolve().parents[3]
        with tempfile.TemporaryDirectory(prefix="mant-tool-package-") as directory:
            package = Path(directory, "scripts")
            package.mkdir()
            (package / "__init__.py").write_text("raise RuntimeError('wrong scripts package')")
            paths = [root / "scripts/rebuild_reference_mandoc.sh",
                     *sorted((root / "scripts/roff/fixtures").glob("regen_*.sh"))]
            self.assertEqual(len(paths), 8)
            for path in paths:
                with self.subTest(entrypoint=path.name):
                    result = subprocess.run(
                        ["bash", str(path), "--help"], cwd=directory,
                        env=reference_environment(), capture_output=True, text=True,
                        timeout=10, check=False,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("usage:", result.stdout)


if __name__ == "__main__":
    unittest.main()
