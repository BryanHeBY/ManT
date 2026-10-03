#!/usr/bin/env python3
"""Check private append counters against actual memory-parser lifetimes."""

import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import tempfile


CRATE = Path(__file__).resolve().parents[2]
VENDOR = CRATE / "vendor" / "mandoc-cvs-20260927T130954Z"
CASES = CRATE / "src" / "tests" / "append_growth" / "cases.json"


def tree_texts(output):
    """Read tree.c::print_attr flags as attributes, not part of a TEXT label."""
    # print_attr emits DELIMO '(', NODE_LINE '*', line:column, DELIMC ')',
    # NODE_EOS '.', then named flags. All belong after '(text) '. They do not
    # change whether this is a native TEXT node (including generated NOSRC).
    pattern = re.compile(r"\(?\*?\d+:\d+\)?\.?(?: .*)?$")
    result = []
    for line in output.splitlines():
        label, marker, attributes = line.rpartition(" (text) ")
        if marker and pattern.fullmatch(attributes):
            result.append(label.lstrip(" "))
    return result


def source_list(path, name):
    """Reuse the build hook's actual native source registry."""
    expression = (r"const\s+" + re.escape(name)
                  + r"\s*:\s*&\[&str\]\s*=\s*&\[(.*?)\];")
    match = re.search(expression, path.read_text(), re.S)
    if match is None:
        raise ValueError(f"cannot read native build source list {name}")
    sources = re.findall(r'"([A-Za-z0-9_]+\.c)"', match.group(1))
    if not sources or len(sources) != len(set(sources)):
        raise ValueError(f"empty or duplicate native source list {name}")
    return sources


def compile_driver(directory):
    system = platform.system()
    target = {
        "Linux": ("linux-gnu.h", "LINUX_COMPAT_SOURCES"),
        "Darwin": ("macos.h", "MACOS_COMPAT_SOURCES"),
    }.get(system)
    if target is None:
        raise ValueError("C maintenance probe requires Linux/glibc or macOS; "
                         "the Windows parser uses the packaged Rust regressions")
    sources = source_list(CRATE / "build.rs", "LIBMANDOC_SOURCES")
    sources += source_list(CRATE / "src" / "build_config.rs", target[1])
    staged = directory / "vendor"
    shutil.copytree(VENDOR, staged)
    shutil.copy2(CRATE / "config" / target[0], directory / "config.h")
    compiler = shlex.split(os.environ.get("CC", "cc"))
    if not compiler:
        raise ValueError("CC must name a compiler")
    binary = directory / "append-growth"
    argv = [*compiler, "-std=c11", "-O2", "-DMANDOC_APPEND_TEST",
            "-Dopen=mant_mandoc_source_open", "-I", str(directory),
            "-I", str(staged), "-I", str(CRATE / "config"),
            "-I", str(CRATE / "shim"), str(CRATE / "tests" / "native" / "append_growth.c"),
            *(str(staged / source) for source in sources), "-lz", "-lm",
            "-o", str(binary)]
    subprocess.run(argv, check=True, timeout=120, cwd=directory)
    return binary


def materialize_cases(directory):
    cases = json.loads(CASES.read_text())
    paths = {}
    for case in cases:
        label = case["id"]
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", label) or label in paths:
            raise ValueError(f"invalid or duplicate case identity {label!r}")
        source = case["source"].encode()
        if hashlib.sha256(source).hexdigest() != case["oracle"]["source_sha256"]:
            raise ValueError(f"source differs from pristine binding: {label}")
        path = directory / f"{label}.1"
        path.write_bytes(source)
        paths[label] = path
    return cases, paths


def parser_metrics(binary, cases, paths):
    result = subprocess.run([str(binary), *(str(path) for path in paths.values())],
                            capture_output=True, text=True, timeout=60, check=True)
    print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="")
    metrics = {}
    pattern = re.compile(r"^(.*?) seed=(\d+) seed_bytes=(\d+) append=(\d+) "
                         r"append_bytes=(\d+) reserve=(\d+) relocation=(\d+) "
                         r"retired=(\d+) retired_bytes=(\d+)$")
    for line in result.stdout.splitlines():
        match = pattern.fullmatch(line)
        if match is None:
            raise ValueError(f"invalid native counter record: {line!r}")
        label = Path(match.group(1)).stem
        if label not in paths or label in metrics or Path(match.group(1)) != paths[label]:
            raise ValueError(f"unbound or duplicate native counter source {label}")
        metrics[label] = dict(zip(("seed", "seed_bytes", "append", "append_bytes",
                                  "reserve", "relocation", "retired", "retired_bytes"),
                                 map(int, match.groups()[1:])))
    if metrics.keys() != paths.keys():
        raise ValueError("actual parser must execute every bound source")
    growth = [case for case in cases if "growth" in case]
    expected_axes = {(family, size) for family in ("man", "mdoc", "tbl")
                     for size in (1024, 8192)}
    if {(case["growth"]["family"], case["growth"]["size"]) for case in growth} != expected_axes:
        raise ValueError("actual growth sources must cover all six parser axes")
    for case in growth:
        axis = case["growth"]
        actual = metrics[case["id"]]
        # Two actual parses. MAN_JOIN B and tbl_cdata are the only append
        # paths in their page headers; mdoc Dd can also append prologue words.
        minimum = 2 * (axis["size"] if axis["family"] == "tbl" else axis["size"] - 1)
        if actual["append"] < minimum or actual["seed"] == 0:
            raise ValueError(f"growth parser path was not exercised: {case['id']}")
        if axis["family"] != "mdoc" and actual["append"] != minimum:
            raise ValueError(f"append count changed: {case['id']}")
        # This rejects an implementation that grows exactly on every append
        # even if helper-only counters happened to satisfy their own bound.
        if actual["reserve"] > actual["seed"] + 32:
            raise ValueError(f"parser reserves are not logarithmic: {case['id']}")
    return metrics


def overflow_checks(binary):
    for flag in ("--overflow-used", "--overflow-source"):
        result = subprocess.run([str(binary), flag], capture_output=True, timeout=5)
        # mandoc_aux.c follows the existing allocation SYSERR exit contract.
        if result.returncode != 6 or not result.stderr:
            raise ValueError(f"checked arithmetic did not reject {flag}: {result.returncode}")


def main():
    # All compiler products, copied source and generated inputs are owned by
    # this scratch scope. No build, patch, configure or make runs in vendor.
    with tempfile.TemporaryDirectory(prefix="mant-append-counters-") as temporary:
        directory = Path(temporary)
        binary = compile_driver(directory)
        cases, paths = materialize_cases(directory)
        parser_metrics(binary, cases, paths)
        overflow_checks(binary)
        print(json.dumps({"status": "pass", "sources": len(cases),
                          "growthAxes": 6, "parserSessionsPerSource": 2,
                          "scope": "two native append paths; per active run"}))


if __name__ == "__main__":
    main()
