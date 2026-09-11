"""Bounded POSIX subprocess boundary for optional local reference audits.

Resource limits are not a filesystem/network sandbox. Use trusted corpora or
an externally confined audit host for adversarial input.
"""
from __future__ import annotations

import math
import os
import selectors
import signal
import subprocess
import sys
import time
from pathlib import Path

MAX_OUTPUT_BYTES = 64 * 1024 * 1024
MAX_INPUT_BYTES = 16 * 1024 * 1024
MAX_ADDRESS_SPACE = 1024 * 1024 * 1024


def reference_environment() -> dict[str, str]:
    # No inherited formatter flags, locale, pager, Python/module injection or
    # loader settings. Callers explicitly supply the resolved manual hierarchy.
    environment = {key: os.environ[key] for key in ("PATH", "TMPDIR") if key in os.environ}
    environment.update({
        "MANWIDTH": "200", "MANROFFOPT": "-rHY=0", "MANPAGER": "cat",
        "PAGER": "cat", "GROFF_NO_SGR": "1", "TERM": "dumb",
        "LC_ALL": "C.UTF-8", "LANG": "C.UTF-8", "TZ": "UTC",
    })
    return environment


def run_renderer(command, timeout, environment, input_bytes=None, *,
                 output_limit=MAX_OUTPUT_BYTES, memory_limit=MAX_ADDRESS_SPACE,
                 binary_output=False):
    empty = b"" if binary_output else ""
    if os.name != "posix":
        return 125, empty, "bounded reference execution requires a POSIX audit host"
    if timeout <= 0 or output_limit <= 0 or memory_limit <= 0:
        return 125, empty, "renderer budgets must be positive"
    if input_bytes is not None and len(input_bytes) > MAX_INPUT_BYTES:
        return 125, empty, "renderer input exceeds 16 MiB"
    launcher = [sys.executable, "-I", str(Path(__file__).resolve()), "--exec",
                str(math.ceil(timeout) + 1), str(memory_limit), *command]
    deadline = time.monotonic() + timeout
    try:
        process = subprocess.Popen(
            launcher, stdin=subprocess.PIPE if input_bytes else subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment,
            start_new_session=True,
        )
    except OSError as error:
        return 125, empty, f"could not start renderer: {error}"
    streams = [bytearray(), bytearray()]
    total = 0
    pending = memoryview(input_bytes or b"")
    try:
        with selectors.DefaultSelector() as selector:
            for index, pipe in enumerate((process.stdout, process.stderr)):
                os.set_blocking(pipe.fileno(), False)
                selector.register(pipe, selectors.EVENT_READ, index)
            if input_bytes:
                os.set_blocking(process.stdin.fileno(), False)
                selector.register(process.stdin, selectors.EVENT_WRITE, None)
            while selector.get_map() or process.poll() is None:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return 124, empty, f"renderer timed out after {timeout}s"
                for key, _ in selector.select(min(remaining, 0.05)):
                    if key.data is None:
                        try:
                            pending = pending[os.write(key.fd, pending[:65536]):]
                        except BrokenPipeError:
                            pending = memoryview(b"")
                        except BlockingIOError:
                            continue
                        if not pending:
                            selector.unregister(key.fileobj)
                            key.fileobj.close()
                        continue
                    try:
                        chunk = os.read(key.fd, min(65536, output_limit - total + 1))
                    except BlockingIOError:
                        continue
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    total += len(chunk)
                    if total > output_limit:
                        return 125, empty, f"renderer output exceeds {output_limit} bytes"
                    streams[key.data].extend(chunk)
        return (process.returncode,
                bytes(streams[0]) if binary_output else streams[0].decode("utf-8", errors="replace"),
                streams[1].decode("utf-8", errors="replace"))
    finally:
        # Kill descendants that inherited our pipes too, even if the leader
        # exited. Deliberately detached processes require an external sandbox.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
        for pipe in (process.stdin, process.stdout, process.stderr):
            if pipe is not None:
                pipe.close()


def execute_limited(arguments):
    # Limits are installed in a fresh interpreter, never in a preexec_fn in
    # the audit parent. exec preserves them for the formatter and its children.
    import resource
    cpu, memory = map(int, arguments[:2])
    for kind, ceiling in [
        (resource.RLIMIT_CPU, cpu), (resource.RLIMIT_AS, memory),
        (resource.RLIMIT_FSIZE, MAX_OUTPUT_BYTES), (resource.RLIMIT_CORE, 0),
    ]:
        _, hard = resource.getrlimit(kind)
        limit = ceiling if hard == resource.RLIM_INFINITY else min(ceiling, hard)
        resource.setrlimit(kind, (limit, limit))
    os.execvpe(arguments[2], arguments[2:], os.environ)


def self_check():
    from unittest.mock import patch
    with patch.dict(os.environ, {"LC_ALL": "unexpected", "GROFFOPT": "bad", "MANOPT": "bad"}):
        environment = reference_environment()
        assert environment["LC_ALL"] == "C.UTF-8"
        assert "GROFFOPT" not in environment and "MANOPT" not in environment
    if os.name != "posix":
        assert run_renderer(["unused"], 1, environment)[0] == 125
        return
    def run(program, **kwargs):
        return run_renderer([sys.executable, "-c", program],
                            kwargs.pop("timeout", 5), environment, **kwargs)
    assert run("import sys; sys.stdout.buffer.write(sys.stdin.buffer.read())",
               input_bytes="café 日本".encode())[1] == "café 日本"
    assert run("import sys; sys.stdout.buffer.write(bytes([255,0,128]))",
               binary_output=True)[1] == b"\xff\0\x80"
    for fd in (1, 2):
        status, output, error = run(f"import os; os.write({fd}, b'x' * 10000)", output_limit=100)
        assert status == 125 and not output and "exceeds" in error
    assert run("import time; time.sleep(5)", timeout=0.2)[0] == 124
    # The direct child exits, but its descendant retains stdout/stderr.
    assert run("import os,time; child=os.fork(); time.sleep(5) if child == 0 else None",
               timeout=0.2)[0] == 124
    status, _, _ = run("allocation = bytearray(512 * 1024 * 1024)", memory_limit=128 * 1024 * 1024)
    assert status != 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["--exec"]:
        execute_limited(sys.argv[2:])
    else:
        self_check()
        print("bounded reference runner self-check succeeded")
