#!/usr/bin/env python3
"""Count annotated-entry function calls in an unoptimized mant debug binary.

This is an A15 diagnostic, not a timing benchmark. GDB software breakpoints
can make a run much slower. Use annotated_perf_baseline.py and its frozen
release binary for wall time/RSS; use this script only to explain repeated
recognition, validation, and index work. The inferior writes to /dev/null.

Examples:

  python3 scripts/annotated_entry_call_counts.py \
    --input tests/fixtures/roff/annotated-man-ip-option-prefixes.1 \
    --route fixed --operation explain --query=--all

  python3 scripts/annotated_entry_call_counts.py \
    --fixture-dir target/entry-en00.oXEsSw

The second form runs the four frozen EN00 decoded pages through Flow and
Fixed text, outline, and explain routes. It does not build or modify them.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
MARKER = "__MANT_ENTRY_CALL_COUNTS_JSON__="
DEFAULT_BINARY = ROOT / "target/debug/mant"
MATRIX_QUERIES = {
    "gcc": "-x",
    "git": "--help",
    "clang": "-help",
    "rclone": "--help",
}

# Exact demangled function names, excluding similarly named closure symbols.
# A missing symbol is an error, not a zero. These are entry counts, not item
# counts: one call may inspect many owners, and one owner may be checked twice.
SYMBOLS = {
    "flow_candidate_head": "mant_codec::definitions::syntax::head::is_inferred_head",
    "flow_identity": "mant_codec::definitions::syntax::infer_identity",
    "flow_name_occurrences": "mant_codec::definitions::syntax::name_occurrences",
    "shared_option_recognition": "mant_ir::entry::syntax::recognition::recognize_option_declarations",
    "fixed_non_option_recognition": "<mant_ir::fixed_body::FixedBody>::scan_non_option_declaration",
    "fixed_lexical_names": "<mant_ir::fixed_body::FixedBody>::lexical_names",
    "fixed_manual_call": "<mant_ir::fixed_body::FixedBody>::manual_call_declaration",
    "fixed_entry_validation": "<mant_ir::fixed_body::FixedBody>::validated_entry",
    "fixed_body_validation": "<mant_ir::fixed_body::FixedBody>::validate",
    "fixed_surface_validation": "<mant_ir::fixed_body::DisplaySurface>::validate",
    "document_snapshot_validation": "<mant_ir::validation::snapshot::DocumentValidation>::new",
    "document_validation": "mant_ir::validation::document::validate_document",
    "semantic_index_build": "<mant_ir::entry::index::SemanticIndex>::build",
    "semantic_index_build_fixed": "<mant_ir::entry::index::SemanticIndex>::build_fixed",
}


def _in_gdb() -> bool:
    try:
        import gdb  # noqa: F401 -- only available inside GDB's Python
    except ImportError:
        return False
    return True


def _gdb_main() -> None:
    import gdb

    counters = {label: 0 for label in SYMBOLS}
    exited: dict[str, Any] = {}

    class Counter(gdb.Breakpoint):
        def __init__(self, label: str, symbol: str) -> None:
            # Quoting prevents C++/Rust punctuation from becoming a location
            # expression. Internal breakpoints suppress per-hit console output.
            super().__init__(f"'{symbol}'", internal=True)
            self.label = label

        def stop(self) -> bool:
            counters[self.label] += 1
            return False

    def on_exit(event: Any) -> None:
        exited["exitCode"] = getattr(event, "exit_code", None)

    gdb.execute("set pagination off", to_string=True)
    gdb.execute("set confirm off", to_string=True)
    gdb.execute("set print thread-events off", to_string=True)
    gdb.execute("set breakpoint pending off", to_string=True)
    gdb.execute("set disable-randomization off", to_string=True)
    gdb.execute("set inferior-tty /dev/null", to_string=True)
    gdb.events.exited.connect(on_exit)
    missing: dict[str, str] = {}
    for label, symbol in SYMBOLS.items():
        try:
            Counter(label, symbol)
        except gdb.error as error:
            missing[label] = str(error)
    if missing:
        print(MARKER + json.dumps({"error": "missing debug symbols", "missing": missing}))
        return
    try:
        gdb.execute("run", to_string=True)
    except gdb.error as error:
        print(MARKER + json.dumps({"error": "inferior run failed", "detail": str(error)}))
        return
    if exited.get("exitCode") != 0:
        print(MARKER + json.dumps({"error": "inferior did not exit successfully", **exited}))
        return
    print(MARKER + json.dumps({"exitCode": 0, "counts": counters}, sort_keys=True))


def _digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def _command(binary: Path, source: Path, route: str, operation: str, query: str) -> list[str]:
    args = [str(binary), "--input", str(source), "--input-format", "roff"]
    if route == "fixed":
        args.insert(1, "--annotated-preview")
    if operation == "text":
        args.extend(("--format", "text", "--display", "direct", "--color", "never"))
    else:
        args.extend(("--format", "json", "--compact"))
        if operation == "outline":
            args.extend(("--outline", "--outline-entries", "all", "--outline-references", "none"))
        else:
            args.append(f"--explain={query}")
    return args


def _case(binary: Path, source: Path, route: str, operation: str, query: str, gdb_bin: str) -> dict[str, Any]:
    command = _command(binary, source, route, operation, query)
    env = os.environ.copy()
    env.update(
        LC_ALL="C.UTF-8", LANG="C.UTF-8", TZ="UTC", TERM="dumb",
        MANWIDTH="78", COLUMNS="78", NO_COLOR="1",
    )
    result = subprocess.run(
        [gdb_bin, "-q", "-nx", "-batch", "-x", str(Path(__file__).resolve()), "--args", *command],
        cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    found = [line[len(MARKER):] for line in result.stdout.splitlines() if line.startswith(MARKER)]
    if result.returncode != 0 or len(found) != 1:
        raise RuntimeError(
            f"GDB failed for {source.name} {route}/{operation}: exit={result.returncode}; "
            f"stdout={result.stdout[-1000:]!r}; stderr={result.stderr[-1000:]!r}"
        )
    measured = json.loads(found[0])
    if "error" in measured:
        detail = ""
        if measured["error"] != "missing debug symbols":
            plain = subprocess.run(
                command, cwd=ROOT, env=env, stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE, text=True, check=False,
            )
            detail = f"; uninstrumented exit={plain.returncode}, stderr={plain.stderr[-1000:]!r}"
        raise RuntimeError(f"GDB failed for {source.name} {route}/{operation}: {measured}{detail}")
    return {
        "source": str(source), "sourceSha256": _digest(source),
        "route": route, "operation": operation,
        "query": query if operation == "explain" else None,
        "exitCode": measured["exitCode"], "counts": measured["counts"],
    }


def _main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--gdb", default="gdb")
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--input", type=Path, help="one decoded roff input")
    selection.add_argument("--fixture-dir", type=Path, help="four-page EN00 decoded input directory")
    parser.add_argument("--route", choices=("flow", "fixed"), help="required with --input")
    parser.add_argument("--operation", choices=("text", "outline", "explain"), help="required with --input")
    parser.add_argument("--query", help="required with --input --operation explain")
    args = parser.parse_args()

    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"debug mant binary is missing or not executable: {binary}")
    if not binary.is_relative_to((ROOT / "target").resolve()):
        parser.error("binary must be inside the repository target directory")
    gdb_bin = shutil.which(args.gdb)
    if gdb_bin is None:
        parser.error(f"GDB executable not found: {args.gdb}")

    cases: list[tuple[Path, str, str, str]] = []
    if args.input is not None:
        if args.route is None or args.operation is None:
            parser.error("--input requires --route and --operation")
        if args.operation == "explain" and not args.query:
            parser.error("explain requires --query")
        cases.append((args.input.resolve(), args.route, args.operation, args.query or ""))
    else:
        if args.route is not None or args.operation is not None or args.query is not None:
            parser.error("--fixture-dir runs both routes and all three operations; omit case selectors")
        folder = args.fixture_dir.resolve()
        for page, query in MATRIX_QUERIES.items():
            source = folder / f"{page}.1"
            for route in ("flow", "fixed"):
                for operation in ("text", "outline", "explain"):
                    cases.append((source, route, operation, query))
    for source, _, _, _ in cases:
        if not source.is_file():
            parser.error(f"decoded roff input does not exist: {source}")

    runs = [_case(binary, source, route, operation, query, gdb_bin)
            for source, route, operation, query in cases]
    print(json.dumps({
        "schemaVersion": 1,
        "measurement": "GDB function-entry counts only; not time or allocation counts",
        "binary": str(binary), "binarySha256": _digest(binary),
        "runs": runs,
    }, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    if _in_gdb():
        _gdb_main()
    else:
        sys.exit(_main())
