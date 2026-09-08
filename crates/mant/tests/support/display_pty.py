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


def terminal_mode(attributes, platform=sys.platform):
    """Compare configuration, excluding only Darwin's pending-input state.

    XNU ttioctl sets PENDIN when ICANON is restored with TCSANOW, then ORs it
    into the requested flags. It is not an un-restored user mode. Do not clear
    it on the terminal (which could discard pending input), or ignore any of
    the actual raw-mode flags, control characters or speeds.
    https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c
    """
    mode = list(attributes)
    if platform == "darwin":
        mode[3] &= ~termios.PENDIN
    return mode


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
                     action=None, returncodes=(0,), diagnostic=None, wait_for_raw=False):
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
            raw = not (termios.tcgetattr(slave)[3] & termios.ICANON)
            if b"\x1b[?1049h" in result and not quit_sent and (not wait_for_raw or raw):
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
        actual = termios.tcgetattr(slave)
        assert terminal_mode(actual) == terminal_mode(original), (
            arguments, "terminal mode leaked", original, actual
        )
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
    styled = root / "styled.md"
    styled.write_text("# Pager\n\n<!-- mant:entries role=option -->\n- `--" + "z" * 256 + "`: Body.\n\n" + "\n\n".join(f"Paragraph {i}." for i in range(20)), encoding="utf-8")
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
    for width in [20, 40, 80, 120]:
        in_session(lambda: check_pager_rows(styled, environment, width))
    for display in ["tui", "pager"]:
        for termination in [signal.SIGINT, signal.SIGTERM]:
            for wait_for_raw in [False, True]:
                in_session(lambda: check_in_session(
                    [sys.argv[1], "--input", str(long), "--display", display],
                    True, environment,
                    action=lambda process, _master: process.send_signal(termination),
                    returncodes=(-termination, 128 + termination),
                    wait_for_raw=wait_for_raw,
                ))
            print("ManT", display, "signal restoration", termination, "passed", flush=True)


def check_pager_rows(path, environment, width):
    """Assert actual SGR on independently emitted continuation glyphs."""
    import re
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 3, width, 0, 0))
    original = termios.tcgetattr(slave)
    os.set_blocking(master, False)
    process = subprocess.Popen([sys.argv[1], "--input", str(path), "--format", "text", "--display", "pager", "--color", "always"], stdin=slave, stdout=slave, stderr=slave, env=environment)
    all_output = bytearray()
    def read_output():
        output = bytearray()
        end = time.monotonic() + 0.18
        while time.monotonic() < end:
            if select.select([master], [], [], 0.02)[0]:
                chunk = read_chunk(master)
                if chunk:
                    output.extend(chunk)
                    # The search prompt asks the terminal for its cursor position.
                    if b"\x1b[6n" in output:
                        os.write(master, b"\x1b[3;1R")
        all_output.extend(output)
        return output.decode("utf-8", "replace")
    def check_colors(text):
        # Start from the prompt's default, not the preceding content row.
        foreground, glyphs = None, 0
        for token in re.findall(r"\x1b\[[0-?]*[ -/]*[@-~]|.", text, re.S):
            if token.startswith("\x1b["):
                if token.endswith("m"):
                    for parameter in token[2:-1].split(";"):
                        code = int(parameter or "0")
                        if code in (0, 39): foreground = None
                        elif 30 <= code <= 37 or 90 <= code <= 97: foreground = code
            elif token == "z":
                assert foreground == 92, (width, text, foreground)
                glyphs += 1
        return glyphs
    try:
        deadline = time.monotonic() + 5
        while b"\x1b[?1049h" not in all_output and time.monotonic() < deadline:
            read_output()
        assert b"\x1b[?1049h" in all_output
        # Scroll forward and backward while a long single name spans rows.
        observed = 0
        for key in [b"j", b"j", b"j", b"k", b"\x04", b"\x15"]:
            os.write(master, key)
            observed += check_colors(read_output())
        assert observed > 0, (width, all_output)
        for columns in [120, 20, width]:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 3, columns, 0, 0))
            process.send_signal(signal.SIGWINCH)
            check_colors(read_output())
        os.write(master, b"/")
        read_output()
        for key in [b"B", b"o", b"d", b"y", b"\r"]:
            os.write(master, key)
            read_output()
        os.write(master, b"n")
        read_output()
        # Validate actual glyph colors while matching wrapped name rows, not
        # merely the presence of a green escape somewhere in the output.
        os.write(master, b"/")
        read_output()
        os.write(master, b"z+\r")
        read_output()  # includes uncolored query-prompt echo
        searched = 0
        for key in [b"n", b"N", b"n"]:
            os.write(master, key)
            searched += check_colors(read_output())
        assert searched > 0, (width, all_output)
        for columns in [20, 80, width]:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 3, columns, 0, 0))
            process.send_signal(signal.SIGWINCH)
            check_colors(read_output())
        # A missing visible query is normal, including ANSI parameter digits.
        # This drives the actual FetchSearchQuery event path (formerly unwrap()).
        for query in (b"NEVER_PRESENT", b"92"):
            os.write(master, b"/")
            read_output()
            os.write(master, query + b"\r")
            read_output()
            assert process.poll() is None, (query, process.returncode, all_output)
            for key in (b"n", b"N"):
                os.write(master, key)
                check_colors(read_output())
                assert process.poll() is None
        # Cancelling a new query clears the search overlay, not source colors.
        os.write(master, b"/")
        read_output()
        os.write(master, b"\x1b")
        check_colors(read_output())
        os.write(master, b"k")
        check_colors(read_output())
        os.write(master, b"q")
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            raise AssertionError((width, all_output[-4000:]))
        read_output()
        assert process.returncode == 0
        assert b"\x1b[?1049l" in all_output
        assert terminal_mode(termios.tcgetattr(slave)) == terminal_mode(original)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        signal.signal(signal.SIGHUP, signal.SIG_IGN)
        os.close(master)
        os.close(slave)


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
    # No ManT code involved: prove that an ordinary raw/cooked round trip is
    # accepted on the host kernel, including Darwin's automatic PENDIN bit.
    in_session(lambda: check_in_session(
        [sys.executable, "-c", (
            "import termios, tty; original = termios.tcgetattr(0); "
            "tty.setraw(0, termios.TCSANOW); "
            "termios.tcsetattr(0, termios.TCSANOW, original)"
        )], False, os.environ
    ))
    print("session lifetime and restoration-negative checks passed", flush=True)


def test_terminal_mode_comparison():
    # Exercise the Darwin exception on every Unix runner. No other flag or
    # attribute difference may disappear, and Linux retains exact comparison.
    original = [0, 0, 0, termios.ICANON | termios.ECHO, 9600, 9600, [b"\x03"]]
    pending = list(original)
    pending[3] |= termios.PENDIN
    assert terminal_mode(original, "darwin") == terminal_mode(pending, "darwin")
    assert terminal_mode(original, "linux") != terminal_mode(pending, "linux")
    assert pending[3] & termios.PENDIN, "comparison mutated its input"
    for index, difference in [
        (0, termios.ICRNL), (1, termios.OPOST), (2, termios.CLOCAL),
        (3, termios.ICANON), (3, termios.ECHO), (3, termios.ISIG),
        (3, termios.IEXTEN), (4, 1), (5, 1),
    ]:
        changed = list(pending)
        changed[index] ^= difference
        assert terminal_mode(changed, "darwin") != terminal_mode(original, "darwin")
    changed = list(pending)
    changed[6] = [b"\x04"]
    assert terminal_mode(changed, "darwin") != terminal_mode(original, "darwin")


if __name__ == "__main__":
    test_terminal_mode_comparison()
    test_drain_boundaries()
    test_session_lifecycle()
    with tempfile.TemporaryDirectory(prefix="mant-display-pty-") as root:
        run_cases(root)
