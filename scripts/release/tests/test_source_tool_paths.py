"""Guard portable checkout paths and source-owned release tool inputs."""

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


def case_collisions(paths):
    """Include directory prefixes: different leaves can share a folded root."""
    entries = {}
    collisions = set()
    for path in paths:
        parts = path.split("/")
        for end in range(1, len(parts) + 1):
            entry = "/".join(parts[:end])
            previous = entries.setdefault(entry.casefold(), entry)
            if previous != entry:
                collisions.add(tuple(sorted((previous, entry))))
    return sorted(collisions)


class SourceToolPathTests(unittest.TestCase):
    def test_case_collisions_include_directories_and_files(self):
        self.assertEqual(
            case_collisions(["LICENSES/CC-BY.txt", "licenses/about.hbs"]),
            [("LICENSES", "licenses")],
        )
        self.assertEqual(case_collisions(["docs/README.md", "docs/readme.md"]),
                         [("docs/README.md", "docs/readme.md")])
        self.assertEqual(case_collisions([
            "LICENSES/CC-BY.txt", "scripts/release/templates/rust-licenses.hbs",
            "crates/libmandoc-rs/LICENSES/ISC.txt",
        ]), [])

    def test_checkout_paths_are_distinct_on_case_insensitive_hosts(self):
        result = subprocess.run(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
            cwd=ROOT, capture_output=True, check=True, timeout=10,
        )
        # Include new files before staging and ignore deleted index entries;
        # build output remains outside Git's source inventory.
        paths = [path for path in os.fsdecode(result.stdout).split("\0")
                 if path and (ROOT / path).exists()]
        self.assertEqual(case_collisions(paths), [])

    @unittest.skipUnless(os.name == "posix" and BASH,
                         "Unix notice generation requires POSIX Bash")
    def test_notice_generation_resolves_its_template_from_another_directory(self):
        with tempfile.TemporaryDirectory(prefix="mant license template ") as directory:
            root = Path(directory)
            tool_bin = root / "bin"
            tool_bin.mkdir()
            cargo = tool_bin / "cargo"
            cargo.write_text("""#!/usr/bin/env bash
set -eu
if [[ $* == 'about --version' ]]; then
    printf 'cargo-about 0.9.2\\n'
    exit
fi
[[ $1 == about && $2 == generate ]]
shift 2
while (( $# )); do
    case $1 in
        --output-file) shift; report=$1 ;;
        *.hbs) template=$1 ;;
    esac
    shift
done
cat "$template" > "$report"
""")
            cargo.chmod(0o755)
            cargo_about = tool_bin / "cargo-about"
            cargo_about.write_text("#!/usr/bin/env bash\nexit 0\n")
            cargo_about.chmod(0o755)
            output = root / "notice.html"
            command = [BASH, str(ROOT / "scripts/release/generate-rust-licenses.sh")]
            env = {**os.environ, "PATH": f"{tool_bin}{os.pathsep}{os.environ['PATH']}"}
            template = ROOT / "scripts/release/templates/rust-licenses.hbs"
            expected = "\n".join(line.rstrip(" \t")
                                 for line in template.read_text().splitlines()) + "\n"
            for arguments in [[str(output)], ["--check", str(output)]]:
                result = subprocess.run(
                    [*command, *arguments], cwd=root, env=env,
                    capture_output=True, text=True, timeout=10, check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output.read_text(), expected)

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
