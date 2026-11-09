"""Finite direct-output checks through actual 40/78-column Unix PTYs."""
import fcntl
import json
import os
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path

ANSI = re.compile(r'\x1b\[[0-9;]*m|\x1b\]8;[^\x1b\x07]*(?:\x1b\\|\x07)')
# Leave time for the per-child cleanup before the Rust caller's 45s deadline.
HARNESS_DEADLINE = time.monotonic() + 38


def interrupted(*_):
    raise KeyboardInterrupt('ordinary interruption')


def capture_direct(arguments, width, environment, *, wait_before_read=False):
    master, slave = pty.openpty()
    child = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, width, 0, 0))
        os.set_blocking(master, False)
        with tempfile.TemporaryFile() as diagnostics:
            child = subprocess.Popen(
                arguments, stdin=subprocess.DEVNULL, stdout=slave, stderr=diagnostics,
                env=environment, start_new_session=True,
            )
            deadline = min(time.monotonic() + 5, HARNESS_DEADLINE)
            if wait_before_read:
                child.wait(timeout=max(0, deadline - time.monotonic()))
            output = bytearray()
            # Darwin ptsclose() calls ttyclose(), which flushes the queued
            # output. Keep our slave open through child exit and the final
            # drain. With the slave still open, completion is exit plus a
            # drained master, so no EOF is required.
            while time.monotonic() < deadline:
                finished = child.poll() is not None
                readable = select.select([master], [], [], 0 if finished else .05)[0]
                if readable:
                    try:
                        data = os.read(master, 65536)
                    except BlockingIOError:
                        data = b''
                    output.extend(data)
                    if data:
                        continue
                if finished:
                    diagnostics.seek(0)
                    return bytes(output), child.returncode, diagnostics.read()
            raise AssertionError(('direct PTY deadline exceeded', arguments, output[-500:]))
    finally:
        try:
            if child is not None and child.poll() is None:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait(timeout=5)
        finally:
            os.close(slave)
            os.close(master)


for sig in [signal.SIGTERM, signal.SIGHUP]:
    signal.signal(sig, interrupted)

environment = dict(os.environ, TERM='xterm-256color')
environment.pop('NO_COLOR', None)

# A short writer can exit before the parent is scheduled to read. Force that
# ordering with a payload that fits in the terminal queue.
witness, status, error = capture_direct(
    [sys.executable, '-c', 'print("PTY-WITNESS")'], 40, environment, wait_before_read=True,
)
assert (witness.replace(b'\r\n', b'\n'), status, error) == (b'PTY-WITNESS\n', 0, b'')

with tempfile.TemporaryDirectory(prefix='mant-table-pty-') as scratch:
    for case in json.loads(sys.argv[2]):
        source = Path(scratch) / 'source.1'
        source.write_bytes(case['source'].encode())
        for width in [40, 78]:
            for color in ['never', 'always']:
                output, status, error = capture_direct(
                    [sys.argv[1], '--input', str(source), '--input-format', 'roff',
                     '--format', 'text', '--display', 'direct', '--color', color],
                    width, environment,
                )
                assert status == 0, (case['id'], error)
                text = ANSI.sub('', output.decode()).replace('\r\n', '\n')
                assert '\x1b' not in text, 'unclosed/unexpected decoration'
                for word in ([case['expected_label'], 'BETA', 'AFTER']
                             if 'expected_label' in case else ['AFTER']):
                    hits = list(re.finditer(r'(?<![\w-])'+re.escape(word)+r'(?![\w-])', text))
                    assert len(hits) == 1, (case['id'], width, color, word, text)
                for fragment in case.get('required_fragments', []):
                    spelling = fragment['text']
                    assert text.count(spelling) == fragment['count'], (case['id'], width, color, fragment, text)
                for recovery in case.get('recoveries', []):
                    assert text.count(recovery['text']) == recovery['occurrences'], (case['id'], width, color, recovery, text)
                assert 'mant:deferred-origin:' not in text
