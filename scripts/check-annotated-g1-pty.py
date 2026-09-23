#!/usr/bin/env python3
"""Exercise annotated-preview pager/TUI terminal lifecycle on a real page.

The checked-in Clang source was run through the pinned CVS reference before
the P1 display assertions. This reuses the product's bounded PTY harness;
it checks terminal restoration and records actual interactive redraw bytes.
The in-process Ratatui tests prove exact hit/reveal/selection coordinates.
"""

from __future__ import annotations

import fcntl
import os
from pathlib import Path
import signal
import struct
import sys
import termios
import time


ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "crates/mant/tests/support"))
import display_pty  # noqa: E402  (shared checked-in test harness)


def resize(process, master: int, columns: int) -> None:
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 16, columns, 0, 0))
    process.send_signal(signal.SIGWINCH)
    time.sleep(0.08)


def pager_action(process, master: int) -> None:
    # The pinned pager maps `l`/Right to horizontal left-mark movement.
    os.write(master, b"lll")
    time.sleep(0.08)
    resize(process, master, 20)
    resize(process, master, 120)
    os.write(master, b"q")


def tui_action(process, master: int) -> None:
    # Shift+Right is the Fixed horizontal key, then resize the true TUI.
    os.write(master, b"\x1b[1;2C")
    time.sleep(0.08)
    resize(process, master, 20)
    resize(process, master, 120)
    os.write(master, b"q")


def run(display: str, action) -> None:
    command = [str(ROOT / "target/release/mant"), "--annotated-preview",
               "--input", str(ROOT / "tests/fixtures/roff/real/archlinux/clang.1.gz"),
               "--input-format", "roff", "--display", display, "--color", "never"]
    if display == "pager":
        command.extend(["--format", "text"])
    env = dict(os.environ, TERM="xterm-256color", NO_COLOR="1")
    output = display_pty.check_in_session(command, True, env, action=action,
                                           wait_for_raw=True)
    assert len(output) > 1000, (display, "no real page redraw")
    print(f"{display}: interaction bytes={len(output)}; terminal restored")


def main() -> None:
    display_pty.in_session(lambda: run("pager", pager_action))
    display_pty.in_session(lambda: run("tui", tui_action))


if __name__ == "__main__":
    main()
