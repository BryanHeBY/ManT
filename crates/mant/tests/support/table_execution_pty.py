"""Finite direct-output checks through actual 40/78-column Unix PTYs."""
import errno
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

def interrupted(*_):
    raise KeyboardInterrupt('ordinary interruption')

for sig in [signal.SIGTERM, signal.SIGHUP]:
    signal.signal(sig, interrupted)

with tempfile.TemporaryDirectory(prefix='mant-table-pty-') as scratch:
    for case in json.loads(sys.argv[2]):
        source = Path(scratch) / 'source.1'
        source.write_bytes(case['source'].encode())
        for width in [40, 78]:
            for color in ['never', 'always']:
                master, slave = pty.openpty()
                fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, width, 0, 0))
                child = None
                output = bytearray()
                try:
                    environment = dict(os.environ, TERM='xterm-256color')
                    environment.pop('NO_COLOR', None)
                    child = subprocess.Popen([sys.argv[1], '--input', str(source),
                        '--input-format', 'roff', '--format', 'text', '--display', 'direct',
                        '--color', color], stdin=subprocess.DEVNULL, stdout=slave,
                        stderr=subprocess.PIPE, env=environment, start_new_session=True)
                    os.close(slave)
                    slave = None
                    deadline = time.monotonic() + 5
                    while True:
                        if time.monotonic() >= deadline:
                            raise AssertionError('direct PTY deadline exceeded')
                        if select.select([master], [], [], .05)[0]:
                            try:
                                data = os.read(master, 65536)
                            except OSError as error:
                                if error.errno != errno.EIO:
                                    raise
                                data = b''
                            if not data:
                                break
                            output.extend(data)
                    _, error = child.communicate(timeout=1)
                    assert child.returncode == 0, (case['id'], error)
                    text = ANSI.sub('', output.decode()).replace('\r\n', '\n')
                    assert '\x1b' not in text, 'unclosed/unexpected decoration'
                    for word in [case['expected_label'], 'BETA', 'AFTER']:
                        hits = list(re.finditer(r'(?<![\w-])'+re.escape(word)+r'(?![\w-])', text))
                        assert len(hits) == 1, (case['id'], width, color, word, text)
                finally:
                    if child is not None:
                        try:
                            os.killpg(child.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        child.wait()
                        if child.stderr is not None:
                            child.stderr.close()
                    os.close(master)
                    if slave is not None:
                        os.close(slave)
