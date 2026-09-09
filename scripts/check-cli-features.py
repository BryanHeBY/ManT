#!/usr/bin/env python3
"""Exercise the supported CLI capability matrix without workspace feature merging."""

import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parent.parent
FEATURES = {"roff", "tui", "pager", "mcp", "update"}
MATRIX = [("minimal", set()), *[(name, {name}) for name in sorted(FEATURES)],
          ("default", FEATURES), ("all", FEATURES)]


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def verify(name, enabled, inputs, log):
    flags = [] if name == "default" else ["--no-default-features"]
    if name == "all":
        flags += ["--all-features"]
    elif enabled and name != "default":
        flags += ["--features", ",".join(sorted(enabled))]
    environment = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "target"))
    command = ["cargo", "build", "--locked", "--package", "mant", *flags,
               "--message-format=json-render-diagnostics"]
    build = subprocess.run(command, cwd=ROOT, env=environment, stdout=subprocess.PIPE,
                           stderr=log, encoding="utf-8", check=True)
    log.write(build.stdout)
    artifacts = [json.loads(line) for line in build.stdout.splitlines()]
    binary = next(Path(item["executable"]) for item in artifacts
                  if item.get("reason") == "compiler-artifact"
                  and item.get("target", {}).get("name") == "mant"
                  and "bin" in item["target"]["kind"] and item.get("executable"))
    tree = subprocess.check_output(
        ["cargo", "tree", "--locked", "--package", "mant", *flags,
         "--edges", "normal,build", "--target", "all", "--prefix", "none", "--format", "{p}"],
        cwd=ROOT, env=environment, encoding="utf-8",
    )
    packages = {line.split()[0] for line in tree.splitlines() if line.strip()}
    for feature, names in {
        "roff": {"libmandoc-rs"},
        "tui": {"mant-ui", "ratatui", "arboard"},
        "pager": {"crossbeam-channel", "textwrap"},
        "mcp": {"rmcp", "rmcp-macros", "tokio"},
        "update": {"ureq", "tar", "zip"},
    }.items():
        if feature in enabled:
            require(names <= packages, f"{name}: missing enabled dependencies {names - packages}")
        else:
            require(not packages.intersection(names), f"{name}: leaked {packages.intersection(names)}")
    if not enabled:
        require(not packages.intersection({"cc", "zstd-sys", "flate2", "crossterm"}),
                f"{name}: minimal build acquired native/terminal dependencies")

    runtime_env = dict(environment, XDG_DATA_HOME=str(inputs), APPDATA=str(inputs),
                       HOME=str(inputs), USERPROFILE=str(inputs), LOCALAPPDATA=str(inputs),
                       XDG_CACHE_HOME=str(inputs / "cache"),
                       MANT_MANPATH=str(inputs / "man"), MANT_TLDR_DIR=str(inputs / "tldr"),
                       TERM="dumb", NO_COLOR="1")

    def run(args, text=None):
        result = subprocess.run([str(binary), *args], cwd=ROOT, env=runtime_env,
                                input=text, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                encoding="utf-8", timeout=30)
        log.write(f"\n$ mant {' '.join(args)}\nstdout ({len(result.stdout)} chars): "
                  f"{result.stdout[:2000]}\nstderr: {result.stderr[:2000]}\n")
        log.flush()
        require("\x1b" not in result.stdout + result.stderr, f"{name}: polluted redirected output")
        return result

    help_result = run(["--help"])
    require(help_result.returncode == 0, f"{name}: help failed")
    for feature, args in {
        "roff": ["--manual", "--man-section"],
        "mcp": ["--mcp"],
        "update": ["--update-docs", "--prune-docs", "--update-tldr", "--dry-run"],
    }.items():
        for flag in args:
            require((flag in help_result.stdout) == (feature in enabled),
                    f"{name}: wrong help for {flag}")
            if feature not in enabled:
                failure = run([flag])
                require(failure.returncode != 0 and "unexpected argument" in failure.stderr,
                        f"{name}: disabled flag accepted: {flag}")

    source = "# Probe\n\n## Options\n\nA Unicode body: café 日本.\n"
    result = run(["--input", "-", "--input-format", "markdown", "--outline", "--format", "json"], source)
    require(result.returncode == 0, f"{name}: Markdown query failed: {result.stderr}")
    require(json.loads(result.stdout), f"{name}: outline was not JSON")
    result = run(["--input", "-", "--input-format", "markdown"], source)
    require(result.returncode == 0 and "café 日本" in result.stdout,
            f"{name}: direct text behavior changed")
    for display in ["tui", "pager"]:
        result = run(["--input", str(inputs / "probe.md"), "--display", display])
        require(result.returncode != 0, f"{name}: interactive display accepted redirected streams")
        if display not in enabled:
            require("invalid value" in result.stderr, f"{name}: disabled display still parsed")
    result = run(["--schema", "all", "--compact"])
    require(result.returncode == 0 and json.loads(result.stdout), f"{name}: schema unavailable")
    require('"roff"' in result.stdout, f"{name}: wire schema was narrowed by features")
    result = run(["--doctor", "--format", "json", "--compact"])
    report = json.loads(result.stdout)
    native = next(check for check in report["checks"] if check["code"] == "runtime.libmandoc")
    require(native["status"] == ("ok" if "roff" in enabled else "info"),
            f"{name}: doctor misreported native capability")
    result = run(["--list", "--kind", "manual", "--format", "json", "--compact"])
    require(result.returncode == 0 and json.loads(result.stdout)["total"] == 1,
            f"{name}: read-only manual inventory must not require native parsing")
    request = {"schema": "mant.request/v0.11", "input": {
        "kind": "file", "path": str(inputs / "probe.1"), "format": "roff"},
        "view": {"kind": "full"}}
    result = run(["--request-json", "--format", "json"], json.dumps(request))
    if "roff" in enabled:
        require(result.returncode == 0 and json.loads(result.stdout), f"{name}: native input failed")
    else:
        require(result.returncode != 0 and "requires the 'roff' feature" in result.stderr,
                f"{name}: wire request did not report disabled native capability")


def main():
    require(not os.environ.get("CARGO_BUILD_TARGET"),
            "CLI feature probes must execute on the host; unset CARGO_BUILD_TARGET")
    logs = ROOT / "target" / "cli-feature-matrix"
    logs.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="mant-feature-inputs-", dir=ROOT / "target") as directory:
        inputs = Path(directory)
        (inputs / "man").mkdir()
        (inputs / "man" / "man1").mkdir()
        (inputs / "man" / "man1" / "probe.1").write_text(
            ".TH PROBE 1\n.SH NAME\nprobe \\- example\n", encoding="utf-8")
        (inputs / "probe.md").write_text("# Probe\n\nA readable document.\n", encoding="utf-8")
        (inputs / "probe.1").write_text(".TH PROBE 1\n.SH NAME\nprobe \\- example\n", encoding="utf-8")
        for name, features in MATRIX:
            print(f"Checking CLI features: {name}", flush=True)
            with (logs / f"{name}.log").open("w", encoding="utf-8") as log:
                verify(name, features, inputs, log)
    print("CLI capability matrix passed (8 builds and process probes)")


if __name__ == "__main__":
    main()
