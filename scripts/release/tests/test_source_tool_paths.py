"""Execute source-owned workflow steps against old and current tag layouts."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
# Git may check the workflow out with CRLF endings on Windows; the commands
# executed below must stay LF so bash does not parse carriage returns.
WORKFLOW = (ROOT / ".github/workflows/release.yml").read_text().replace("\r\n", "\n")


# Windows resolves a bare "bash" to the System32 WSL placeholder (the loader
# searches System32 before PATH), and that placeholder executes no command
# string. Resolve the interpreter once and keep it only when it runs commands.
def usable_bash():
    path = shutil.which("bash")
    if path is None:
        return None
    probe = subprocess.run(
        [path, "-c", "mant_bash=usable; printf '%s' \"$mant_bash\""],
        capture_output=True, text=True, timeout=10, check=False,
    )
    return path if probe.returncode == 0 and probe.stdout == "usable" else None


BASH = usable_bash()
BASH_STEPS = {
    "Package Unix release archive": "scripts/release/package-release.sh",
    "Package portable manual archive": "scripts/release/package-manuals.sh",
    "Install native publishing dependencies": "scripts/ci/install-ci-native-dependencies.sh",
    "Publish the selected crate versions": "scripts/release/publish-crates.sh",
}


def workflow_command(name):
    step = WORKFLOW.split(f"      - name: {name}\n", 1)[1].split("      - name:", 1)[0]
    lines = []
    for line in step.split("        run: |\n", 1)[1].splitlines():
        if line.strip() and not line.startswith("          "):
            break
        lines.append(line[10:])
    return "\n".join(lines)


class SourceToolPathTests(unittest.TestCase):
    def check_bash_layout(self, layout):
        # A manual retry checks out an immutable tag: old tags only have the
        # root scripts, while current tags own the organized tools. Execute the
        # actual workflow command with harmless stand-ins, never publish/build.
        for name, current in BASH_STEPS.items():
            with self.subTest(step=name, layout=layout), tempfile.TemporaryDirectory(
                prefix="mant release source "
            ) as directory:
                # Shells spell the working directory differently per platform,
                # so the stand-ins read this marker relative to their working
                # directory instead of printing $PWD.
                marker = Path(directory, "checkout-directory")
                marker.write_bytes(marker.parent.name.encode())
                for kind in layout:
                    path = Path(current if kind == "current" else f"scripts/{Path(current).name}")
                    tool = Path(directory, path)
                    tool.parent.mkdir(parents=True, exist_ok=True)
                    tool.write_bytes(
                        (f"printf '%s\\n' '{kind}' \"$MANT_RELEASE_TAG\"\n"
                         "cat checkout-directory\n").encode()
                    )
                result = subprocess.run(
                    [BASH, "-e", "-c", workflow_command(name)], cwd=directory,
                    env={**os.environ, "MANT_RELEASE_TAG": "v0.11.0"},
                    capture_output=True, text=True, timeout=10, check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                expected = "current" if "current" in layout else "legacy"
                self.assertEqual(
                    result.stdout.splitlines(),
                    [expected, "v0.11.0", marker.parent.name],
                )

    @unittest.skipUnless(BASH, "Bash is required for Unix workflow steps")
    def test_old_tag_steps_use_the_source_owned_root_tools(self):
        self.check_bash_layout(["legacy"])

    @unittest.skipUnless(BASH, "Bash is required for Unix workflow steps")
    def test_current_tag_steps_prefer_the_organized_source_tools(self):
        self.check_bash_layout(["current"])
        self.check_bash_layout(["current", "legacy"])

    def test_windows_step_selects_the_tags_own_tool(self):
        command = workflow_command("Package Windows release archive")
        self.assertIn('$Script = "./scripts/release/package-release.ps1"', command)
        self.assertIn("if (-not (Test-Path $Script -PathType Leaf))", command)
        self.assertIn('$Script = "./scripts/package-release.ps1"', command)
        self.assertIn("& $Script", command)
        self.assertNotIn(".release-automation", command)

    @unittest.skipUnless(shutil.which("pwsh"), "Native PowerShell is required")
    def test_windows_steps_run_both_source_layouts(self):
        for layout in [["legacy"], ["current"], ["current", "legacy"]]:
            with self.subTest(layout=layout), tempfile.TemporaryDirectory(
                prefix="mant release source "
            ) as directory:
                for kind in layout:
                    path = ("scripts/release/package-release.ps1" if kind == "current"
                            else "scripts/package-release.ps1")
                    tool = Path(directory, path)
                    tool.parent.mkdir(parents=True, exist_ok=True)
                    tool.write_text(f"Write-Output '{kind}'\nWrite-Output $env:MANT_RELEASE_TAG\n")
                result = subprocess.run(
                    ["pwsh", "-NoProfile", "-NonInteractive", "-Command",
                     workflow_command("Package Windows release archive")],
                    cwd=directory, env={**os.environ, "MANT_RELEASE_TAG": "v0.11.0"},
                    capture_output=True, text=True, timeout=10, check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                expected = "current" if "current" in layout else "legacy"
                self.assertEqual(result.stdout.splitlines(), [expected, "v0.11.0"])


if __name__ == "__main__":
    unittest.main()
