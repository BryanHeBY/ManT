"""Exercise dependency setup without sudo, package downloads or system writes."""

import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "scripts/ci/install-ci-native-dependencies.sh"


@unittest.skipUnless(os.name == "posix", "Unix dependency setup")
class NativeDependencyTests(unittest.TestCase):
    def run_setup(self, *, primary_failure="", fallback_failure=False,
                  ready=False, architecture="amd64", distro="ubuntu"):
        with tempfile.TemporaryDirectory(prefix="mant native setup ") as directory:
            root = Path(directory)
            tools = root / "tools"
            tools.mkdir()
            scratch = root / "scratch"
            scratch.mkdir()
            marker = root / "installed"
            if ready:
                marker.touch()
            release = root / "os-release"
            release.write_text(f'ID={distro}\nVERSION_CODENAME="noble"\n')

            def tool(name, source):
                path = tools / name
                path.write_text(source)
                path.chmod(0o755)

            tool("cc", '#!/bin/sh\ncat >/dev/null\ntest -f "$MANT_TEST_STATE/installed"\n')
            for name in ["ar", "cvs"]:
                tool(name, "#!/bin/sh\nexit 0\n")
            tool("sudo", '#!/bin/sh\nexec "$@"\n')
            tool("timeout", '#!/bin/sh\nshift 3\nexec "$@"\n')
            tool("dpkg", '#!/bin/sh\nprintf "%s\\n" "$MANT_TEST_ARCH"\n')
            awk = shlex.quote(shutil.which("awk"))
            tool("awk", f'''#!/bin/bash
args=("$@")
if [[ ${{args[-1]}} == /etc/os-release ]]; then
  args[-1]="$MANT_TEST_STATE/os-release"
fi
exec {awk} "${{args[@]}}"
''')
            tool("apt-get", '''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
root = Path(os.environ["MANT_TEST_STATE"])
args = sys.argv[1:]
phase = "update" if "update" in args else "install"
sources = next((a.split("=", 1)[1] for a in args if a.startswith("Dir::Etc::sourcelist=")), None)
call = {"phase": phase, "fallback": sources is not None, "args": args}
if sources:
    call["sources"] = Path(sources).read_text()
with (root / "calls.jsonl").open("a") as log:
    log.write(json.dumps(call) + "\\n")
if sources and os.environ["MANT_TEST_FALLBACK_FAIL"] == "1":
    sys.exit(100)
if not sources and phase == os.environ["MANT_TEST_PRIMARY_FAIL"]:
    sys.exit(124)
if phase == "install":
    (root / "installed").touch()
''')
            result = subprocess.run(
                ["bash", str(SCRIPT)], capture_output=True, text=True, timeout=15,
                env={**os.environ, "PATH": f"{tools}{os.pathsep}{os.environ['PATH']}",
                     "TMPDIR": str(scratch), "MANT_TEST_STATE": str(root),
                     "MANT_TEST_ARCH": architecture,
                     "MANT_TEST_PRIMARY_FAIL": primary_failure,
                     "MANT_TEST_FALLBACK_FAIL": "1" if fallback_failure else "0"},
                check=False,
            )
            calls = root / "calls.jsonl"
            records = [json.loads(row) for row in calls.read_text().splitlines()] if calls.exists() else []
            self.assertEqual(list(scratch.iterdir()), [], "probe/fallback files were not cleaned")
            return result, records, marker.exists()

    def test_ready_toolchain_never_runs_apt(self):
        result, calls, installed = self.run_setup(ready=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [])
        self.assertTrue(installed)

    def test_default_success_does_not_change_sources(self):
        result, calls, installed = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([(c["phase"], c["fallback"]) for c in calls],
                         [("update", False), ("install", False)])
        self.assertTrue(installed)

    def test_update_timeout_uses_signed_https_without_installing_from_stale_indexes(self):
        result, calls, installed = self.run_setup(primary_failure="update")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([(c["phase"], c["fallback"]) for c in calls],
                         [("update", False), ("update", True), ("install", True)])
        self.assertTrue(installed)
        for call in calls[1:]:
            self.assertIn("Dir::Etc::sourceparts=-", call["args"])
            self.assertIn("signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg", call["sources"])
            self.assertIn("https://archive.ubuntu.com/ubuntu noble", call["sources"])
            self.assertIn("https://security.ubuntu.com/ubuntu noble-security", call["sources"])
            self.assertNotIn("http://", call["sources"])

    def test_install_timeout_retries_both_operations(self):
        result, calls, installed = self.run_setup(primary_failure="install")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([(c["phase"], c["fallback"]) for c in calls],
                         [("update", False), ("install", False), ("update", True), ("install", True)])
        self.assertTrue(installed)

    def test_arm64_uses_ports_archive(self):
        result, calls, installed = self.run_setup(primary_failure="update", architecture="arm64")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("https://ports.ubuntu.com/ubuntu-ports noble", calls[1]["sources"])
        self.assertNotIn("archive.ubuntu.com", calls[1]["sources"])
        self.assertTrue(installed)

    def test_fallback_failure_stops_without_claiming_dependencies(self):
        result, calls, installed = self.run_setup(primary_failure="update", fallback_failure=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 2)
        self.assertFalse(installed)

    def test_non_ubuntu_failure_does_not_select_an_incompatible_archive(self):
        result, calls, installed = self.run_setup(primary_failure="update", distro="debian")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 1)
        self.assertFalse(installed)


if __name__ == "__main__":
    unittest.main()
