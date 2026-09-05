"""Bounded PTY regression cases; no host manuals or external pager executables."""

import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path


def check(arguments, expected_interactive, env, stdin=None):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 16, 70, 0, 0))
    original = termios.tcgetattr(slave)

    def controlling_terminal():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

    process = subprocess.Popen(
        [sys.argv[1], *arguments],
        stdin=slave if stdin is None else subprocess.PIPE,
        stdout=slave,
        stderr=subprocess.PIPE,
        env=env,
        preexec_fn=controlling_terminal,
    )
    result = bytearray()
    quit_sent = False
    try:
        if stdin is not None:
            process.stdin.write(stdin)
            process.stdin.close()
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.05)[0]:
                result.extend(os.read(master, 65536))
            if b"\x1b[?1049h" in result and not quit_sent:
                # The queued key is consumed after initial drawing completes.
                os.write(master, b"q")
                quit_sent = True
            if process.poll() is not None:
                while select.select([master], [], [], 0)[0]:
                    result.extend(os.read(master, 65536))
                break
        else:
            raise AssertionError(f"display timed out: {arguments}: {result[-500:]!r}")
        diagnostics = process.stderr.read()
        assert process.returncode == 0, (arguments, process.returncode, diagnostics)
        assert not diagnostics, (arguments, diagnostics)
        interactive = b"\x1b[?1049h" in result
        assert interactive == expected_interactive, (arguments, result[:500])
        assert termios.tcgetattr(slave) == original, (arguments, "terminal mode leaked")
        if interactive:
            assert b"\x1b[?1049l" in result, (arguments, "alternate screen not restored")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        process.stderr.close()
        os.close(master)
        os.close(slave)


with tempfile.TemporaryDirectory(prefix="mant-display-pty-") as root:
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
        ("wrapped text", wrapped, ["--format", "text"], True),
        ("direct text", long, ["--display", "direct"], False),
        ("markdown", long, ["--format", "markdown"], False),
        ("markdown pager", long, ["--format", "markdown", "--display", "pager"], True),
        ("automatic reader", long, [], True),
    ]:
        check(["--input", str(path), *extra], interactive, environment)
        print(name, "passed")
    check(["--input", str(long)], False, dict(environment, TERM="dumb"))
    check(["--input", "-", "--input-format", "markdown"], False, environment, b"# Stdin\n\nBody.\n")
    check(["--help"], False, environment)
