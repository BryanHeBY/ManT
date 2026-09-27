#!/usr/bin/env python3
"""Measure one CLI process's public glibc allocation calls without Cargo.

Builds the companion LD_PRELOAD library only under the repository target/,
then runs the exact command once normally and once with the library. It
requires identical exit status, stdout, and stderr. The library reports over
a dedicated inherited file descriptor, never the measured program's streams.
A result is *process-wide*
allocator-ABI traffic, not Rust-only allocation, native collector self-cost,
live heap, or a phase timing. Counting stops at normal library destruction;
abnormal exits have no report. See the C source for coverage limits.

Example:

  python3 scripts/annotated_allocation_probe.py --out target/a15-small.json -- \
    target/debug/mant --annotated-preview \
    --input docs/architecture/baselines/annotated-entry-en00/inputs/roles.1 \
    --input-format roff --format json --compact --explain=counter
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import select
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
TARGET = (ROOT / "target").resolve()
SOURCE = Path(__file__).with_suffix(".c")
MAX_REPORT_BYTES = 4096
REPORT_EOF_TIMEOUT_SECONDS = 5


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def identity(data: bytes) -> dict[str, str | int]:
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def build_library(compiler: str) -> tuple[Path, list[str]]:
    build_root = TARGET / "entry-a15-allocation-probe"
    build_root.mkdir(parents=True, exist_ok=True)
    build_dir = Path(tempfile.mkdtemp(prefix="run-", dir=build_root))
    library = build_dir / "libmant_a15_alloc.so"
    command = [
        compiler, "-std=c11", "-O2", "-fPIC", "-shared", "-Wall", "-Wextra", "-Werror",
        "-o", str(library), str(SOURCE), "-ldl", "-pthread",
    ]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(
            f"A15 probe compilation failed ({result.returncode}): "
            f"{result.stderr.decode('utf-8', 'replace')[-2000:]}"
        )
    return library, command


def read_report(fd: int) -> dict[str, Any]:
    # The C report is under 2 KiB, so subprocess.run cannot block on the
    # pipe while the parent is capturing normal stdout/stderr. Wait for EOF
    # afterward to reject a child that inherited the report descriptor.
    chunks = []
    length = 0
    deadline = time.monotonic() + REPORT_EOF_TIMEOUT_SECONDS
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([fd], [], [], remaining)[0]:
            raise RuntimeError("A15 report FD remained open after the CLI exited")
        chunk = os.read(fd, 4096)
        if not chunk:
            break
        length += len(chunk)
        if length > MAX_REPORT_BYTES:
            raise RuntimeError("A15 allocation report exceeds its bounded size")
        chunks.append(chunk)
    raw = b"".join(chunks)
    if raw.count(b"\n") != 1 or not raw.endswith(b"\n"):
        raise RuntimeError("expected exactly one complete A15 allocation report")
    try:
        report = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("invalid A15 allocation report") from error
    if report.get("schemaVersion") != 1 or not isinstance(report.get("operations"), dict):
        raise RuntimeError("unexpected A15 allocation report schema")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, help="new JSON report path under target/")
    parser.add_argument("--cc", default="cc", help="C compiler; output still stays in target/")
    parser.add_argument("command", nargs=argparse.REMAINDER, help="-- target/mant [arguments]")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.libc_ver()[0] != "glibc":
        parser.error("this LD_PRELOAD probe requires Linux/glibc")
    if os.environ.get("LD_PRELOAD"):
        parser.error("remove the existing LD_PRELOAD before running this probe")
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("supply -- target/mant [arguments]")
    binary = (ROOT / command[0]).resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"not an executable file: {binary}")
    if not binary.is_relative_to(TARGET):
        parser.error("the measured binary must be frozen under repository target/")
    command[0] = str(binary)
    compiler = shutil.which(args.cc)
    if compiler is None:
        parser.error(f"C compiler unavailable: {args.cc}")
    out = None
    if args.out is not None:
        out = (ROOT / args.out).resolve()
        if out == TARGET or not out.is_relative_to(TARGET):
            parser.error("--out must be a new JSON file below repository target/")
        if out.exists():
            parser.error(f"refusing to overwrite existing report: {out}")

    library, build_command = build_library(compiler)
    env = os.environ.copy()
    env.update(
        LC_ALL="C.UTF-8", LANG="C.UTF-8", TZ="UTC", TERM="dumb",
        MANWIDTH="78", COLUMNS="78", NO_COLOR="1",
    )
    baseline = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, check=False)
    instrumented_env = env.copy()
    instrumented_env["LD_PRELOAD"] = str(library)
    read_fd, write_fd = os.pipe()
    try:
        try:
            instrumented_env["MANT_A15_ALLOC_FD"] = str(write_fd)
            instrumented = subprocess.run(
                command, cwd=ROOT, env=instrumented_env, capture_output=True,
                pass_fds=(write_fd,), check=False,
            )
        finally:
            os.close(write_fd)
        counts = read_report(read_fd)
    finally:
        os.close(read_fd)
    if (
        baseline.returncode != instrumented.returncode
        or baseline.stdout != instrumented.stdout
        or baseline.stderr != instrumented.stderr
    ):
        raise RuntimeError(
            "LD_PRELOAD changed CLI exit status, stdout, or stderr: "
            f"plain={baseline.returncode}/{identity(baseline.stdout)}/{identity(baseline.stderr)}, "
            f"preloaded={instrumented.returncode}/{identity(instrumented.stdout)}/"
            f"{identity(instrumented.stderr)}"
        )
    if baseline.returncode != 0:
        raise RuntimeError(f"the measured command failed with status {baseline.returncode}")
    if counts.get("counterOverflow") is not False:
        raise RuntimeError("A15 allocation counter overflow or missing overflow status")

    report = {
        "schemaVersion": 1,
        "measurement": (
            "one fresh process; dynamically interposed public glibc allocator calls and "
            "successful requested bytes through normal library destruction; includes startup "
            "and probe initialization; report uses a dedicated inherited FD; "
            "not stage time, live heap, actual allocation capacity, or all allocations"
        ),
        "limits": [
            "Direct mmap, internal libc allocation, and custom allocators can bypass these hooks.",
            "free.calls includes free(NULL); free.successes and free.requestedBytes are always zero.",
            "The report is emitted during normal library destruction, before any later shutdown work.",
            "A child inheriting the report FD causes multiple reports or an open-FD error; "
            "a child without it is not counted as part of this process.",
            "Rust and C allocations are not separated; requested bytes count realloc's new size.",
            "An abnormal _exit or signal does not run the reporting destructor.",
        ],
        "glibc": platform.libc_ver()[1],
        "compiler": compiler,
        "buildCommand": build_command,
        "probeSource": str(SOURCE),
        "probeSourceSha256": digest(SOURCE),
        "library": str(library),
        "librarySha256": digest(library),
        "binary": str(binary),
        "binarySha256": digest(binary),
        "command": command,
        "environment": {
            name: env[name]
            for name in ("LC_ALL", "LANG", "TZ", "TERM", "MANWIDTH", "COLUMNS", "NO_COLOR")
        },
        "baseline": {
            "exitCode": baseline.returncode,
            "stdout": identity(baseline.stdout),
            "stderr": identity(baseline.stderr),
        },
        "preloaded": {
            "exitCode": instrumented.returncode,
            "stdout": identity(instrumented.stdout),
            "stderr": identity(instrumented.stderr),
        },
        "stdoutStderrAndExitIdentical": True,
        "allocation": counts,
    }
    serialized = json.dumps(report, indent=2, ensure_ascii=False) + "\n"
    if out is not None:
        out.parent.mkdir(parents=True, exist_ok=True)
        with out.open("x", encoding="utf-8") as stream:
            stream.write(serialized)
        print(f"wrote {out}")
    else:
        sys.stdout.write(serialized)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as error:
        print(f"A15 allocation probe: {error}", file=sys.stderr)
        raise SystemExit(1) from None
