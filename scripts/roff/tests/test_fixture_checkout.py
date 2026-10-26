"""Git checkout must preserve the exact source/profile fixture byte bindings."""

import hashlib
import itertools
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = Path("crates/mant-engine/tests/roff_lowering")


class FixtureCheckoutTests(unittest.TestCase):
    def checkout(self, directory, files, attributes, *, autocrlf, eol):
        repository = Path(directory, "repository")
        repository.mkdir()
        destination = Path(directory, "checkout")
        destination.mkdir()
        environment = dict(os.environ, GIT_CONFIG_NOSYSTEM="1",
                           GIT_CONFIG_GLOBAL=os.devnull)
        for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE",
                     "GIT_CONFIG_COUNT", "GIT_CONFIG_PARAMETERS"):
            environment.pop(name, None)

        def git(*arguments, timeout=30):
            subprocess.run(
                ["git", "-c", f"core.autocrlf={str(autocrlf).lower()}",
                 "-c", f"core.eol={eol}",
                 "-c", "core.safecrlf=false", "-c", f"core.attributesFile={os.devnull}",
                 *arguments], cwd=repository, env=environment,
                capture_output=True, check=True, timeout=timeout,
            )

        git("init", "--quiet")
        (repository / ".gitattributes").write_bytes(attributes)
        for path, content in files.items():
            target = repository / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
        # Stage and check out the complete matrix in every configuration. The
        # thousand-plus tiny files can exceed 30 seconds on Windows runners;
        # give these bulk I/O operations a bounded integration-test budget.
        git("add", ".", timeout=120)
        git("checkout-index", "--all", f"--prefix={destination.as_posix()}/", timeout=120)
        return destination

    def test_checkout_keeps_sources_and_sealed_rows_byte_identical(self):
        files = {path.relative_to(ROOT): path.read_bytes()
                 for path in (ROOT / FIXTURES).rglob("*")
                 if path.suffix in {".1", ".expected"}}
        for family in ("literal_leading_rows", "table_layout"):
            files.update({path.relative_to(ROOT): path.read_bytes()
                          for path in (ROOT / "tests/fixtures/roff" / family).rglob("*.1")})
        self.assertTrue(files)
        # This unprotected file proves that the scratch checkout really applies
        # autocrlf/core.eol, independently of the host's native line endings.
        control = Path("unprotected.txt")
        files[control] = b"one\ntwo\n"
        migrations = json.loads(
            (ROOT / FIXTURES / "oracle_migrations/breakpoints.json").read_bytes())
        for autocrlf, eol in itertools.product((False, True), ("lf", "crlf")):
            with self.subTest(autocrlf=autocrlf, eol=eol), tempfile.TemporaryDirectory(
                    prefix="mant-fixture-checkout-") as directory:
                checkout = self.checkout(directory, files, (ROOT / ".gitattributes").read_bytes(),
                                         autocrlf=autocrlf, eol=eol)
                self.assertEqual((checkout / control).read_bytes(),
                                 b"one\r\ntwo\r\n" if autocrlf or eol == "crlf"
                                 else files[control])
                for path, content in files.items():
                    if path != control:
                        self.assertEqual((checkout / path).read_bytes(), content, str(path))
                for case in migrations["cases"]:
                    if case["cohort"] == "hang":
                        continue  # This source is stored in JSON rather than a .1 file.
                    family = {"escape": "escape_matrix", "shared": "shared_execution_matrix"}
                    path = FIXTURES / family[case["cohort"]] / "cases" / (case["name"] + ".1")
                    source = (checkout / path).read_bytes()
                    self.assertEqual(source, case["source"].encode(), str(path))
                    self.assertEqual(hashlib.sha256(source).hexdigest(), case["sourceSha256"])


if __name__ == "__main__":
    unittest.main()
