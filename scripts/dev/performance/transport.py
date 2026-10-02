"""Serial process collection, preserving failed outputs and operation samples."""

import json
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import threading
import time

from .cards import file_identity, process_environment
from .statistics import finite_nonnegative


def resource_tool():
    tool = Path("/usr/bin/time")
    if platform.system() != "Linux" or not tool.is_file():
        return None
    result = subprocess.run([str(tool), "--version"], capture_output=True, check=False)
    if result.returncode or b"GNU" not in result.stdout:
        return None
    return {**file_identity(tool), "version": result.stdout.decode(errors="replace").strip()}


def operation_metrics(payload):
    """Seven rounds are one process sample; never seven independent pairs."""
    if payload.get("schema") != "mant.operation-measurement/v1":
        raise ValueError("operation harness schema missing or unsupported; use the same harness source for both revisions")
    if type(payload.get("rounds")) is not int or payload["rounds"] != 7:
        raise ValueError("operation harness must report exactly seven rounds")
    phase = payload.get("operation") == "phase"
    stages = {"parseOwned", "sourceLessLoweringAndRecognition", "nativeTextRender",
              "sourceLessDrop", "sourceAwareBytesLoad"}
    scope_keys = {"source", "initialLoad", *stages, "combination"} if phase else {
        "source", "initialLoad", "resultAllocation", "resultDestruction", "serialization", "index", "warmup"}
    scope = payload.get("scope")
    if not isinstance(scope, dict) or set(scope) != scope_keys or any(
            not isinstance(value, str) or not value.strip() for value in scope.values()):
        raise ValueError("operation scope does not match the registered harness")
    vectors = payload.get("phaseMilliseconds") if payload.get("operation") == "phase" else {"operation": payload.get("milliseconds")}
    if not isinstance(vectors, dict) or not vectors:
        raise ValueError("missing operation timing vectors")
    if phase and set(vectors) != stages:
        raise ValueError("missing or unknown phase timing vector")
    metrics = {}
    for name, values in vectors.items():
        if not isinstance(values, list) or len(values) != 7 or not all(map(finite_nonnegative, values)):
            raise ValueError(f"{name}: expected seven finite nonnegative rounds")
        metrics[f"operation.{name}.wallMs"] = statistics.median(values[2:])
    return metrics


def collect(argv, destination, stem, *, panel, timeout, collector=None, capture=False):
    validate_timeout(timeout)
    stdout = destination / f"{stem}.stdout"
    stderr = destination / f"{stem}.stderr"
    usage = destination / f"{stem}.usage"
    actual = argv
    if collector:
        actual = [collector["path"], "-f", "%U %S %M", "-o", str(usage), "--", *argv]
    record = {"argv": argv, "actualArgv": actual, "timeoutSeconds": timeout,
              "stdoutDestination": str(stdout) if capture or panel == "operation" else os.devnull,
              "stderrPath": str(stderr), "startedUnixNs": time.time_ns(), "metrics": {}}
    start = time.perf_counter_ns()
    process = None
    try:
        with (stdout if capture or panel == "operation" else Path(os.devnull)).open("wb") as out, stderr.open("wb") as err:
            process = subprocess.Popen(actual, stdout=out, stderr=err, env=process_environment(), start_new_session=os.name == "posix")
            code, expired = wait_with_deadline(process, timeout)
            record.update(status="timeout" if expired else "ok" if code == 0 else "exit-error", exitCode=code)
    except OSError as error:
        record.update(status="spawn-error", error=str(error), exitCode=None)
    record["finishedUnixNs"] = time.time_ns()
    record["metrics"]["process.wallMs"] = (time.perf_counter_ns() - start) / 1_000_000
    if stderr.exists():
        record["stderr"] = file_identity(stderr)
    if stdout.exists() and (capture or panel == "operation"):
        record["stdout"] = file_identity(stdout)
    if collector and usage.exists():
        record["resourceRaw"] = file_identity(usage)
        try:
            parts = usage.read_text().strip().split()
            if len(parts) != 3:
                raise ValueError("expected user, system, RSS triple")
            user, system, rss = map(float, parts)
            if not all(map(finite_nonnegative, (user, system, rss))):
                raise ValueError("non-finite resource value")
            record["metrics"].update({"process.userCpuMs": user * 1000, "process.systemCpuMs": system * 1000,
                                       "process.cpuMs": (user + system) * 1000, "process.peakRssKiB": rss})
        except (ValueError, OSError) as error:
            record["resourceError"] = str(error)
            if record["status"] == "ok":
                record["status"] = "resource-error"
    elif collector and record["status"] == "ok":
        record.update(status="resource-error", resourceError="resource collector produced no record")
    if record["status"] == "ok" and panel == "operation":
        try:
            payload = json.loads(stdout.read_bytes())
            record["operationRaw"] = payload
            if payload.get("operation") != argv[-2]:
                raise ValueError("operation response does not match requested mode")
            record["metrics"].update(operation_metrics(payload))
        except (ValueError, TypeError, AttributeError, OSError) as error:
            record.update(status="operation-error", operationError=str(error))
    return record


def wait_with_deadline(process, timeout):
    """Blocking wait avoids POSIX wait(timeout)'s exponential polling sleeps."""
    validate_timeout(timeout)
    expired = threading.Event()

    def deadline():
        if process.returncode is not None:
            return
        try:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
            expired.set()
        except ProcessLookupError:
            # It already exited while the deadline callback was dispatched.
            pass

    watchdog = threading.Timer(timeout, deadline)
    watchdog.daemon = True
    watchdog.start()
    try:
        code = process.wait()
    finally:
        watchdog.cancel()
        watchdog.join()
    return code, expired.is_set()


def validate_timeout(timeout):
    """Reject deadlines that the platform watchdog cannot actually enforce."""
    if not finite_nonnegative(timeout) or timeout == 0 or timeout > threading.TIMEOUT_MAX:
        raise ValueError("timeout must be finite, positive and within the platform watchdog limit")
