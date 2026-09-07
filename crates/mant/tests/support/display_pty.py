"""Bounded PTY regression cases; no host manuals or external pager executables."""

import errno
import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import traceback
from pathlib import Path


def read_chunk(fd):
    """Nonblocking PTYs signal closure with EOF on macOS or EIO on Linux."""
    try:
        return os.read(fd, 65536)
    except BlockingIOError:
        return None
    except OSError as error:
        if error.errno == errno.EIO:
            return b""
        raise


def drain(fd, output, deadline):
    """Drain queued bytes after exit, never spin on EOF or outlive the case."""
    while True:
        if time.monotonic() >= deadline:
            raise AssertionError("PTY drain timed out")
        chunk = read_chunk(fd)
        if not chunk:
            return
        output.extend(chunk)


def in_session(callback):
    """Keep a disposable session leader alive through all post-child checks.

    Darwin revokes a controlling terminal when its session leader exits. The
    command must therefore be a child of this supervisor, not the leader itself.
    The outer harness never acquires a controlling terminal or changes signals.
    """
    supervisor = os.fork()
    if supervisor == 0:
        try:
            os.setsid()
            callback()
        except BaseException:
            traceback.print_exc()
            sys.stderr.flush()
            os._exit(1)
        sys.stdout.flush()
        os._exit(0)
    reaped = False
    try:
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            finished, status = os.waitpid(supervisor, os.WNOHANG)
            if finished:
                reaped = True
                assert os.waitstatus_to_exitcode(status) == 0, (
                    "PTY session supervisor failed", status
                )
                return
            time.sleep(0.02)
        raise AssertionError("PTY session supervisor timed out")
    finally:
        if not reaped:
            # The group contains only this case's supervisor and command.
            try:
                os.killpg(supervisor, signal.SIGKILL)
            except ProcessLookupError:
                os.kill(supervisor, signal.SIGKILL)
            os.waitpid(supervisor, 0)


def check(arguments, expected_interactive, env, stdin=None):
    in_session(lambda: check_in_session(
        [sys.argv[1], *arguments], expected_interactive, env, stdin
    ))


def check_in_session(arguments, expected_interactive, env, stdin=None,
                     action=None, returncodes=(0,), diagnostic=None):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 16, 70, 0, 0))
    original = termios.tcgetattr(slave)
    os.set_blocking(master, False)

    # A file cannot fill a pipe and deadlock the child before it exits.
    diagnostics_file = tempfile.TemporaryFile()
    process = subprocess.Popen(
        arguments,
        stdin=slave if stdin is None else subprocess.PIPE,
        stdout=slave,
        stderr=diagnostics_file,
        env=env,
    )
    result = bytearray()
    quit_sent = False
    eof = False
    try:
        if stdin is not None:
            process.stdin.write(stdin)
            process.stdin.close()
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if select.select([] if eof else [master], [], [], 0.05)[0]:
                chunk = read_chunk(master)
                if chunk == b"":
                    eof = True
                elif chunk is not None:
                    result.extend(chunk)
            if b"\x1b[?1049h" in result and not quit_sent:
                # The queued key is consumed after initial drawing completes.
                if action is None:
                    os.write(master, b"q")
                else:
                    action(process, master)
                quit_sent = True
            if process.poll() is not None:
                drain(master, result, deadline)
                break
        else:
            diagnostics_file.seek(0)
            print("timeout diagnostics:", diagnostics_file.read(), file=sys.stderr)
            raise AssertionError(f"display timed out: {arguments}: {result[-500:]!r}")
        diagnostics_file.seek(0)
        diagnostics = diagnostics_file.read()
        assert process.returncode in returncodes, (arguments, process.returncode, diagnostics)
        if diagnostic is None:
            assert not diagnostics, (arguments, diagnostics)
        else:
            assert diagnostic in diagnostics, (arguments, diagnostics)
        interactive = b"\x1b[?1049h" in result
        assert interactive == expected_interactive, (arguments, result[:500])
        assert termios.tcgetattr(slave) == original, (arguments, "terminal mode leaked")
        if interactive:
            assert b"\x1b[?1049l" in result, (arguments, "alternate screen not restored")
        return bytes(result)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        diagnostics_file.close()
        # Closing our own controlling PTY can send SIGHUP. Only ignore it after
        # the command has exited, so the command retains normal signal behavior.
        signal.signal(signal.SIGHUP, signal.SIG_IGN)
        os.close(master)
        os.close(slave)


def run_cases(root):
    root = Path(root)
    short = root / "short.md"
    long = root / "long.md"
    wrapped = root / "wrapped.md"
    short.write_text("# Short\n\nBody.\n", encoding="utf-8")
    long.write_text("# Long\n\n" + "\n\n".join(f"Paragraph {i}." for i in range(80)), encoding="utf-8")
    wrapped.write_text("# Wrapped\n\n" + "wide text " * 200, encoding="utf-8")
    environment = dict(os.environ, TERM="xterm-256color", NO_COLOR="1")
    environment.pop("CLICOLOR_FORCE", None)
    for name, path, extra, interactive in [
        ("short text", short, ["--format", "text"], False),
        ("long text", long, ["--format", "text"], True),
        ("coloured text", long, ["--format", "text", "--color", "always"], True),
        ("wrapped text", wrapped, ["--format", "text"], True),
        ("direct text", long, ["--display", "direct"], False),
        ("markdown", long, ["--format", "markdown"], False),
        ("markdown pager", long, ["--format", "markdown", "--display", "pager"], True),
        ("automatic reader", long, [], True),
    ]:
        check(["--input", str(path), *extra], interactive, environment)
        print(name, "passed", flush=True)
    check(["--input", str(long)], False, dict(environment, TERM="dumb"))
    check(["--input", "-", "--input-format", "markdown"], False, environment, b"# Stdin\n\nBody.\n")
    check(["--help"], False, environment)
    for termination in [signal.SIGINT, signal.SIGTERM]:
        in_session(lambda: check_in_session(
            [sys.argv[1], "--input", str(long)], True, environment,
            action=lambda process, _master: process.send_signal(termination),
            returncodes=(-termination, 128 + termination),
        ))
        print("ManT signal restoration", termination, "passed", flush=True)


def test_drain_boundaries():
    # Exercise both OS closure conventions even when CI runs on only one OS.
    from unittest.mock import patch

    for ending in [b"", OSError(errno.EIO, "closed"), BlockingIOError()]:
        with patch("os.read", side_effect=[b"last output", ending]) as reader:
            output = bytearray()
            drain(-1, output, time.monotonic() + 5)
            assert output == b"last output"
            assert reader.call_count == 2
    with patch("os.read", return_value=b"still readable"), patch(
        "time.monotonic", side_effect=[0, 1, 2]
    ):
        try:
            drain(-1, bytearray(), 2)
        except AssertionError as error:
            assert str(error) == "PTY drain timed out"
        else:
            raise AssertionError("continuously readable output escaped its deadline")
    with patch("os.read", side_effect=OSError(errno.EBADF, "bad descriptor")):
        try:
            read_chunk(-1)
        except OSError as error:
            assert error.errno == errno.EBADF
        else:
            raise AssertionError("unexpected read error was swallowed")


def test_session_lifecycle():
    def final_output():
        program = (
            "import os; "
            "assert os.getsid(0) != os.getpid(); "
            "assert os.tcgetpgrp(0) == os.getpgrp(); "
            "tty = os.open('/dev/tty', os.O_RDWR); os.close(tty); "
            "print('x' * 200000 + 'FINAL', end='', flush=True)"
        )
        output = check_in_session([sys.executable, "-c", program], False, os.environ)
        assert output == b"x" * 200000 + b"FINAL", "final PTY output lost"

    def leaked_mode():
        program = (
            "import termios; attrs = termios.tcgetattr(0); "
            "attrs[3] ^= termios.ECHO; "
            "termios.tcsetattr(0, termios.TCSANOW, attrs)"
        )
        try:
            check_in_session([sys.executable, "-c", program], False, os.environ)
        except AssertionError as error:
            assert "terminal mode leaked" in str(error), error
        else:
            raise AssertionError("supervisor concealed a terminal restoration failure")

    in_session(final_output)
    in_session(leaked_mode)
    print("session lifetime and restoration-negative checks passed", flush=True)


if __name__ == "__main__":
    test_drain_boundaries()
    test_session_lifecycle()
    with tempfile.TemporaryDirectory(prefix="mant-display-pty-") as root:
        run_cases(root)
