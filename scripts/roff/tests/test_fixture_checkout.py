"""Git checkout must preserve the exact source/profile fixture byte bindings."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = Path("crates/mant-engine/tests/roff_lowering")


class FixtureCheckoutTests(unittest.TestCase):
    def checkout(self, directory, files, attributes, *, autocrlf):
        repository = Path(directory, "repository")
        repository.mkdir()
        destination = Path(directory, "checkout")
        destination.mkdir()
        environment = dict(os.environ, GIT_CONFIG_NOSYSTEM="1",
                           GIT_CONFIG_GLOBAL=os.devnull)
        for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
            environment.pop(name, None)

        def git(*arguments):
            subprocess.run(
                ["git", "-c", f"core.autocrlf={str(autocrlf).lower()}",
                 "-c", "core.safecrlf=false", "-c", f"core.attributesFile={os.devnull}",
                 *arguments], cwd=repository, env=environment,
                capture_output=True, check=True, timeout=30,
            )

        git("init", "--quiet")
        (repository / ".gitattributes").write_bytes(attributes)
        for path, content in files.items():
            target = repository / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
        git("add", ".")
        git("checkout-index", "--all", f"--prefix={destination.as_posix()}/")
        return destination

    def test_checkout_keeps_sources_and_sealed_rows_byte_identical(self):
        files = {path.relative_to(ROOT): path.read_bytes()
                 for path in (ROOT / FIXTURES).rglob("*")
                 if path.suffix in {".1", ".expected"}}
        self.assertTrue(files)
        # This unprotected file proves that the scratch checkout really applies
        # autocrlf, even when this test runs on a Unix host.
        control = Path("unprotected.txt")
        files[control] = b"one\ntwo\n"
        migrations = json.loads(
            (ROOT / FIXTURES / "oracle_migrations/breakpoints.json").read_bytes())
        for autocrlf in (False, True):
            with self.subTest(autocrlf=autocrlf), tempfile.TemporaryDirectory(
                    prefix="mant-fixture-checkout-") as directory:
                checkout = self.checkout(directory, files, (ROOT / ".gitattributes").read_bytes(),
                                         autocrlf=autocrlf)
                self.assertEqual((checkout / control).read_bytes(),
                                 b"one\r\ntwo\r\n" if autocrlf else files[control])
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
