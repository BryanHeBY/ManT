"""Pristine identity checks and raw process transport for roff fixtures.

Each recorder retains its own arguments, environment, accepted statuses,
decoding and projection. This module never chooses snapshot content.
"""

from pathlib import Path
import subprocess

from scripts.roff.oracle import mandoc_oracle
from scripts.roff.oracle import rebuild_reference_mandoc
def verified_reference(root, binary):
    """Verify the active identity in every required preflight profile."""
    root = Path(root).resolve()
    binary = Path(binary)
    if not binary.is_absolute():
        binary = root / binary
    attestation, registration = rebuild_reference_mandoc.active_attestation(root)
    archive = mandoc_oracle.repository_path(
        root, registration["source"]["archive"]["path"], "source archive")
    rebuild_reference_mandoc.verify_all(
        root, binary, archive, attestation, registration["identity"])
    return registration


def run_reference(binary, arguments, *, input_bytes=None, env=None,
                  timeout=15, check=False):
    """Return undecoded stdout/stderr and the actual process status."""
    return subprocess.run(
        [str(binary), *arguments], input=input_bytes, capture_output=True,
        timeout=timeout, env=env, check=check)
