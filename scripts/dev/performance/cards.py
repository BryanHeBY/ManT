"""Source and process identities for comparable measurements."""

import gzip
import hashlib
import os
from pathlib import Path
import platform
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]

FIXTURES = {
    "gcc": ("fedora44/gcc.1.zst", "7a1ec76f50702b05c389ba777c5dee70155c05781f05056a9dafb07154625699", "531ca21b660c1fbd294218e921d834adfaccd61c6f400a2a5d844759f40fc034"),
    "git": ("fedora44/git.1.zst", "43e55d11719d6b4db16e1da7228e0446a64f4cef4a0fb449ea6c84f8e37f2e03", "2ee1c5dd84a69dfc91d84840e943e7df0ee06b86bf27eb4bb9369c415f3351c1"),
    "clang": ("fedora44/clang.1.zst", "6f2673fa1cee4e1c7a81638645458cd16e1af5af05644e174d026d87c6c38228", "8f727b7a3966a90989f474259bab124fdb3935913dffcb6912c1237a63b2b241"),
    "rclone": ("windows-releases/rclone.1.zst", "4e38c8e1a35e13faafda1d8bfa14b25f792e665476b7405d541e8462e049286e", "f35de3b3008f684a7db141a7626db68b08c46e5b3d45c97726bc6d97eebade4e"),
}

CLI_MODES = {
    "text": ["--format", "text"],
    "markdown": ["--format", "markdown"],
    "outline": ["--outline", "--format", "json", "--compact"],
    "explain": ["--explain=-h", "--format", "json", "--compact"],
    "search": ["--search", "option", "--format", "json", "--compact"],
}
OPERATION_MODES = ("load", "index", "outline", "explain", "text", "markdown", "search", "phase")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def file_identity(path):
    path = Path(path).resolve()
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return {"path": str(path), "sha256": digest.hexdigest(), "bytes": path.stat().st_size}


def fixed_input(name, directory, *, root=ROOT):
    relative, stored_hash, raw_hash = FIXTURES[name]
    source = root / "tests/fixtures/roff/real" / relative
    stored = source.read_bytes()
    if sha256(stored) != stored_hash:
        raise ValueError(f"fixed fixture stored hash changed: {source}")
    decoded = subprocess.run(["zstd", "-dc", str(source)], capture_output=True, check=False)
    if decoded.returncode:
        raise ValueError(f"zstd decode failed ({decoded.returncode}): {decoded.stderr!r}")
    if sha256(decoded.stdout) != raw_hash:
        raise ValueError(f"fixed fixture raw hash changed: {source}")
    gzip_path = directory / f"{name}.1.gz"
    # This container is an operation-only input. Original CLI zstd and CRLF
    # bytes remain unchanged, and its hash is independently registered.
    with gzip_path.open("wb") as output:
        with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as encoder:
            encoder.write(decoded.stdout)
    return {
        "id": name,
        "stored": file_identity(source),
        "raw": {"sha256": raw_hash, "bytes": len(decoded.stdout)},
        "operationInput": file_identity(gzip_path),
        "transformation": "unchanged zstd member bytes recompressed to gzip, filename empty, mtime=0",
        "provenance": file_identity(source.parent / "README.md"),
    }


def process_environment():
    return {**os.environ, "LC_ALL": "C.UTF-8", "TERM": "dumb", "COLUMNS": "78", "LINES": "24"}


def host_card():
    def read(path):
        try:
            return Path(path).read_text().strip()
        except OSError:
            return None
    return {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "logicalCpus": os.cpu_count(),
        "cpuInfo": read("/proc/cpuinfo"),
        "governor": read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        "powerControl": "not changed by harness; record external policy in build/measurement card",
        "locale": "C.UTF-8", "term": "dumb",
        "terminalEnvironment": {"COLUMNS": "78", "LINES": "24", "interactiveViewport": False},
        "fileCache": "hot after process warmups; no drop_caches or cold-cache claim",
    }


def command(binary, input_card, panel, mode):
    if panel == "cli":
        return [str(binary), "--input", input_card["stored"]["path"],
                "--input-format", "roff", "--color", "never", "--display", "direct",
                *CLI_MODES[mode]]
    literal = "option" if mode == "search" else "-h"
    return [str(binary), input_card["operationInput"]["path"], mode, literal]


def measurement_scope(panel, mode):
    return {
        "operationWall": (
            "new process through exit including actual I/O, decode, load, query, encode, writes and destruction; stdout /dev/null"
            if panel == "cli" else
            "one process owns seven operation timers; keep all seven, median last five is one process-level operation sample"
        ),
        "processResources": "whole child process user/system CPU and peak RSS, including initial load/decode and untimed destruction; not operation-exclusive",
        "processWall": "parent spawn-to-blocking-wait, including deadline Timer creation/cancel/join and GNU time wrapper if enabled; no wait(timeout) polling",
        "query": "-h" if mode == "explain" else "option" if mode == "search" else None,
        "outlineDetail": "default Summary" if mode == "outline" else None,
        "searchPagination": "protocol defaults: offset 0, limit 100, visible literal insensitive" if mode == "search" else None,
        "outputHash": "independent CLI correctness invocation; not timed process output hashing",
        "phase": "source-less parse/owned/lower+recognition/render/drop and independent source-aware bytes loader; not CPU profiling" if mode == "phase" else None,
    }


def build_card_admission(card, artifacts):
    """Never infer an old artifact's producer from the current checkout."""
    required = ("revision", "compiler", "nativeCompiler", "profile", "features", "cargoLockSha256")
    if not isinstance(card, dict):
        return {"status": "incomplete", "missingFields": list(required),
                "invalidFields": ["build-card"], "unboundArtifacts": list(artifacts)}
    missing = [name for name in required if name not in card]
    invalid = [name for name in required[:4]
               if name in card and (not isinstance(card[name], str) or not card[name].strip())]
    features = card.get("features")
    if "features" in card and not (isinstance(features, list)
                                   and all(isinstance(value, str) and value.strip() for value in features)):
        invalid.append("features")
    if "cargoLockSha256" in card and not (isinstance(card["cargoLockSha256"], str)
                                          and re.fullmatch(r"[0-9a-f]{64}", card["cargoLockSha256"])):
        invalid.append("cargoLockSha256")
    mismatches = []
    recorded_artifacts = card.get("artifacts")
    if not isinstance(recorded_artifacts, dict):
        recorded_artifacts = {}
    for panel, artifact in artifacts.items():
        recorded = recorded_artifacts.get(panel, {})
        if not isinstance(recorded, dict) or recorded.get("sha256") != artifact["sha256"]:
            mismatches.append(panel)
    if "operation" in artifacts and card.get("operationHarnessSha256") != operation_harness_identity()["sha256"]:
        mismatches.append("operation-harness-source")
    return {"status": "bound" if not missing and not invalid and not mismatches else "incomplete",
            "missingFields": missing, "invalidFields": invalid, "unboundArtifacts": mismatches,
            "attestation": "caller-supplied build record; this harness does not build or authenticate compiler output"}


def operation_harness_identity():
    paths = [ROOT / "crates/mant-engine/examples/measure_native_load.rs",
             ROOT / "crates/mant-engine/examples/support/operation_measurement.rs",
             ROOT / "crates/mant-engine/examples/support/phase_measurement.rs"]
    files = {str(path.relative_to(ROOT)): file_identity(path) for path in paths}
    # Only relative paths and contents participate: copying this source into a
    # retained revision's scratch build must not change its harness identity.
    canonical = "\n".join(f"{name}\t{files[name]['sha256']}" for name in sorted(files))
    return {"sha256": sha256(canonical.encode()), "files": files}
