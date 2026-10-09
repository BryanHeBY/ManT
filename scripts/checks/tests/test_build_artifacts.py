"""Keep one-shot caches small without skipping archive-source verification."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from scripts.checks.build_environment import verification_environment


ROOT = Path(__file__).resolve().parents[3]
PACKAGES = ["mant-ir", "mant-protocol", "libmandoc-rs", "mant-sources", "mant-codec",
            "mant-loader", "mant-query", "mant-render", "mant-engine", "mant-ui", "mant"]
BASH = shutil.which("bash") if os.name == "posix" else None
CARGO = shutil.which("cargo")
PWSH = shutil.which("pwsh")

FAKE_CARGO = r'''#!/usr/bin/env python3
import io
import json
import os
from pathlib import Path
import signal
import shutil
import sys
import tarfile

root = Path(os.environ["BUILD_FIXTURE_ROOT"])
args = sys.argv[1:]
scenario = os.environ.get("BUILD_FIXTURE_SCENARIO", "success")
with (root / "calls.jsonl").open("a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\n")
command = args[0]
if command == "pkgid":
    package = args[args.index("-p") + 1]
    print(f"path+file://{root}/crates/{package}#{package}@0.1.0")
elif command == "package":
    if scenario == "package-failure":
        sys.exit(9)
    package = args[args.index("-p") + 1]
    artifacts = Path(args[args.index("--target-dir") + 1])
    assert artifacts.parent.name.startswith("mant-package-check.")
    assert os.environ["CARGO_TARGET_DIR"] == str(root / "target")
    destination = artifacts / "package" / f"{package}-0.1.0.crate"
    destination.parent.mkdir(parents=True, exist_ok=True)
    files = {"Cargo.toml": b"[package]\n", "Cargo.toml.orig": b"[package]\n"}
    if package == "mant":
        files.update({"tests/support/display_pty.py": b"# fixture\n",
                      "src/delivery/pager/vendor/LICENSE-APACHE": b"fixture\n",
                      "src/delivery/pager/vendor/LICENSE-MIT": b"fixture\n"})
    with tarfile.open(destination, "w:gz") as archive:
        for name, content in files.items():
            info = tarfile.TarInfo(f"{package}-0.1.0/{name}")
            info.size = len(content)
            archive.addfile(info, io.BytesIO(content))
    staging = destination.parent / "tmp-crate" / destination.name
    staging.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(destination, staging)
elif command == "test":
    assert os.environ["CARGO_INCREMENTAL"] == "0"
    assert os.environ["CARGO_PROFILE_DEV_DEBUG"] == "line-tables-only"
    assert os.environ["CARGO_PROFILE_TEST_DEBUG"] == "line-tables-only"
    manifest = Path(args[args.index("--manifest-path") + 1])
    assert manifest.parent.name.startswith("mant-package-check.")
    (root / "target/debug/deps/snapshot-workspace.bin").write_bytes(b"disposable")
    if scenario in ("test-failure", "both-failures"):
        sys.exit(7)
    if scenario == "interrupt":
        os.kill(os.getppid(), signal.SIGTERM)
elif command == "clean":
    assert args[args.index("--profile") + 1] == "dev"
    assert Path(args[args.index("--manifest-path") + 1]) == root / "Cargo.toml"
    assert args.count("--package") == 11
    if scenario in ("cleanup-failure", "both-failures"):
        sys.exit(8)
    (root / "target/debug/deps/snapshot-workspace.bin").unlink(missing_ok=True)
else:
    raise AssertionError(args)
'''


class VerificationEnvironmentTests(unittest.TestCase):
    def test_defaults_are_scoped_and_disable_incremental_even_when_requested(self):
        base = {"CARGO_INCREMENTAL": "1", "UNRELATED": "retained"}
        environment = verification_environment(base)
        self.assertEqual(base, {"CARGO_INCREMENTAL": "1", "UNRELATED": "retained"})
        self.assertEqual(environment["CARGO_INCREMENTAL"], "0")
        self.assertEqual(environment["CARGO_PROFILE_DEV_DEBUG"], "line-tables-only")
        self.assertEqual(environment["CARGO_PROFILE_TEST_DEBUG"], "line-tables-only")
        self.assertEqual(environment["UNRELATED"], "retained")

    def test_explicit_debug_preferences_are_preserved(self):
        environment = verification_environment({"CARGO_PROFILE_DEV_DEBUG": "2",
                                                "CARGO_PROFILE_TEST_DEBUG": "0"})
        self.assertEqual(environment["CARGO_PROFILE_DEV_DEBUG"], "2")
        self.assertEqual(environment["CARGO_PROFILE_TEST_DEBUG"], "0")

    @unittest.skipUnless(BASH, "shell environment requires POSIX Bash")
    def test_shell_and_python_defaults_agree_without_changing_caller_environment(self):
        base = dict(os.environ)
        for name in ("CARGO_INCREMENTAL", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"):
            base.pop(name, None)
        base.update(CARGO_INCREMENTAL="1", CARGO_PROFILE_DEV_DEBUG="2")
        result = subprocess.run(
            [BASH, "-c", 'source "$1"; printf "%s\\n" "$CARGO_INCREMENTAL" '
             '"$CARGO_PROFILE_DEV_DEBUG" "$CARGO_PROFILE_TEST_DEBUG"', "fixture",
             str(ROOT / "scripts/checks/build-environment.sh")],
            env=base, capture_output=True, text=True, timeout=10, check=True,
        )
        self.assertEqual(result.stdout.splitlines(), ["0", "2", "line-tables-only"])
        self.assertEqual(base["CARGO_INCREMENTAL"], "1")


@unittest.skipUnless(BASH, "packaged-source wrapper requires POSIX Bash")
class PackagedCleanupTests(unittest.TestCase):
    def run_fixture(self, scenario, expected, repeats=1):
        with tempfile.TemporaryDirectory(prefix="mant build [case] ") as directory:
            root = Path(directory)
            checks = root / "scripts/checks"
            checks.mkdir(parents=True)
            for name in ("check-packaged-crates.sh", "build-environment.sh"):
                shutil.copyfile(ROOT / "scripts/checks" / name, checks / name)
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            (root / "Cargo.lock").write_text("# fixture\n", encoding="utf-8")
            (root / "bin").mkdir()
            cargo = root / "bin/cargo"
            cargo.write_text(FAKE_CARGO, encoding="utf-8")
            cargo.chmod(0o755)
            retained = [root / "target/debug/deps/third-party.rlib",
                        root / "target/release/mant",
                        root / "target/package/user-kept.crate",
                        root / "target/mandoc-migration/reference/mandoc"]
            for path in retained:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"retained")
            environment = dict(os.environ, PATH=str(root / "bin") + os.pathsep + os.environ["PATH"],
                               BUILD_FIXTURE_ROOT=str(root), BUILD_FIXTURE_SCENARIO=scenario,
                               CARGO_TARGET_DIR=str(root / "ignored-target-override"),
                               CARGO_INCREMENTAL="1", CARGO_PROFILE_DEV_DEBUG="line-tables-only",
                               CARGO_PROFILE_TEST_DEBUG="line-tables-only")
            for _ in range(repeats):
                result = subprocess.run([BASH, str(checks / "check-packaged-crates.sh")],
                                        env=environment, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertEqual("packaged crate verification succeeded" in result.stdout,
                                 expected == 0)
                self.assertEqual(list((root / "target").glob("mant-package-check.*")), [])
                self.assertEqual(list((root / "target/package").glob("*.crate")),
                                 [root / "target/package/user-kept.crate"])
                for path in retained:
                    self.assertEqual(path.read_bytes(), b"retained")
            calls = [json.loads(line) for line in (root / "calls.jsonl").read_text().splitlines()]
            clean = [call for call in calls if call[0] == "clean"]
            if scenario == "package-failure":
                self.assertEqual(clean, [])
            else:
                self.assertEqual(len(clean), repeats)
                self.assertEqual([clean[0][index + 1]
                                  for index, value in enumerate(clean[0]) if value == "--package"],
                                 PACKAGES)
                if scenario not in ("cleanup-failure", "both-failures"):
                    self.assertFalse((root / "target/debug/deps/snapshot-workspace.bin").exists())

    def test_success_and_repeated_runs_leave_only_reusable_cache(self):
        self.run_fixture("success", 0, repeats=2)

    def test_failed_packaging_removes_partial_snapshot_without_cleaning_debug_cache(self):
        self.run_fixture("package-failure", 9)

    def test_failed_tests_keep_failure_and_clean_disposable_artifacts(self):
        self.run_fixture("test-failure", 7)

    def test_ordinary_interruption_cleans_snapshot_and_artifacts(self):
        self.run_fixture("interrupt", 143)

    def test_cleanup_failure_is_reported_but_scratch_is_still_removed(self):
        self.run_fixture("cleanup-failure", 8)
        self.run_fixture("both-failures", 7)


class CargoArtifactTests(unittest.TestCase):
    @unittest.skipUnless(CARGO, "artifact ownership probe requires Cargo")
    def test_real_cargo_rebuilds_snapshot_then_cleans_only_workspace_debug_artifacts(self):
        with tempfile.TemporaryDirectory(prefix="mant-cargo-artifacts-") as directory:
            root = Path(directory)
            dependency = root / "dependency"
            dependency.joinpath("src").mkdir(parents=True)
            dependency.joinpath("Cargo.toml").write_text(
                '[package]\nname="cache-fixture-dep"\nversion="0.1.0"\nedition="2021"\n',
                encoding="utf-8")
            dependency.joinpath("src/lib.rs").write_text("pub fn base() -> u32 { 1 }\n",
                                                       encoding="utf-8")
            checkout = root / "checkout"
            checkout.joinpath("src").mkdir(parents=True)
            checkout.joinpath("Cargo.toml").write_text(
                '[package]\nname="disk-fixture"\nversion="0.1.0"\nedition="2021"\n'
                '[workspace]\n[dependencies]\ncache-fixture-dep={path='
                + json.dumps(dependency.as_posix()) + '}\n', encoding="utf-8")
            checkout.joinpath("src/lib.rs").write_text(
                "pub fn value() -> u32 { cache_fixture_dep::base() }\n", encoding="utf-8")
            target = root / "target"
            environment = verification_environment()
            environment["CARGO_TARGET_DIR"] = str(target)
            # Explicit cross-target settings belong to the invoking user, not
            # this host-only compiler-ownership fixture.
            environment.pop("CARGO_BUILD_TARGET", None)

            def cargo(arguments):
                return subprocess.run([CARGO, *arguments], env=environment,
                                      capture_output=True, text=True, timeout=120, check=True)

            build = cargo(["build", "--offline", "--manifest-path", str(checkout / "Cargo.toml"),
                           "--message-format=json"])
            artifacts = [json.loads(line) for line in build.stdout.splitlines()]
            shared = next(item["filenames"] for item in artifacts
                          if item.get("reason") == "compiler-artifact"
                          and item["target"]["name"] == "cache_fixture_dep")
            snapshot = root / "snapshot"
            shutil.copytree(checkout, snapshot)
            snapshot.joinpath("src/lib.rs").write_text(
                "pub fn value() -> u32 { cache_fixture_dep::base() + 1 }\n"
                "#[test] fn snapshot_is_not_checkout_binary() { assert_eq!(value(), 2); }\n",
                encoding="utf-8")
            result = cargo(["test", "--offline", "--locked", "--manifest-path",
                            str(snapshot / "Cargo.toml"), "--message-format=json"])
            self.assertIn("snapshot_is_not_checkout_binary ... ok", result.stdout)
            own = []
            for line in result.stdout.splitlines():
                if line.startswith("{"):
                    item = json.loads(line)
                    if item.get("reason") == "compiler-artifact" and item["target"]["name"] == "disk_fixture":
                        own.extend(item["filenames"])
            self.assertTrue(own)
            release = target / "release/retained"
            oracle = target / "mandoc-migration/reference/mandoc"
            for path in (release, oracle):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"retained")
            cargo(["clean", "--offline", "--locked", "--manifest-path",
                   str(checkout / "Cargo.toml"), "--profile", "dev", "--package", "disk-fixture"])
            self.assertTrue(all(Path(path).is_file() for path in shared))
            self.assertTrue(all(not Path(path).exists() for path in own))
            self.assertEqual(release.read_bytes(), b"retained")
            self.assertEqual(oracle.read_bytes(), b"retained")
            self.assertEqual(list(target.joinpath("debug/incremental").glob("*")), [])

    @unittest.skipUnless(PWSH, "PowerShell restore check requires pwsh")
    def test_windows_gate_restores_build_environment_on_early_failure(self):
        with tempfile.TemporaryDirectory(prefix="mant-pwsh-build-") as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.joinpath("checks").mkdir(parents=True)
            shutil.copyfile(ROOT / "scripts/check-windows.ps1", scripts / "check-windows.ps1")
            scripts.joinpath("install.ps1").write_text("# parser fixture\n", encoding="utf-8")
            scripts.joinpath("checks/check-windows-paths.ps1").write_text(
                "throw 'fixture failure'\n", encoding="utf-8")
            command = (
                "$env:CARGO_INCREMENTAL='1'; $env:CARGO_PROFILE_DEV_DEBUG='2'; "
                "$env:CARGO_PROFILE_TEST_DEBUG='0'; try { & '"
                + str(scripts / "check-windows.ps1").replace("'", "''")
                + "' } catch { if ($_ -notmatch 'fixture failure') { throw } }; "
                "@($env:CARGO_INCREMENTAL,$env:CARGO_PROFILE_DEV_DEBUG,"
                "$env:CARGO_PROFILE_TEST_DEBUG) | ConvertTo-Json -Compress"
            )
            result = subprocess.run([PWSH, "-NoProfile", "-Command", command], capture_output=True,
                                    text=True, timeout=30, check=True)
            self.assertEqual(json.loads(result.stdout.splitlines()[-1]), ["1", "2", "0"])


if __name__ == "__main__":
    unittest.main()
