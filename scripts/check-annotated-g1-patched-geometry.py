#!/usr/bin/env python3
"""Compare every P1 Fixed CLI body row with the approved patched raw renderer.

First run the ignored `emit_patched_raw_renderer_for_four_page_display_comparison`
libmandoc test. That test writes only under the repository target directory.
This audit folds known terminal overstrike with the existing conservative
scanner, verifies the four fixtures' exact two-row furniture shape, and then
compares the normalized visible body including spaces and native row breaks.
Raw escape and overstrike bytes are deliberately outside this comparison. It
does not claim pristine CVS had the same approved-patch wrapping.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess

from roff_content_compare import visible_text


ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "target/annotated-g1-patched-raw"
CLI = ROOT / "target/release/mant"
FIXTURES = (
    ("GCC", "gcc.1", "archlinux/gcc.1.gz", "GCC(1)"),
    ("Git", "git.1", "archlinux/git.1.gz", "GIT(1)"),
    ("Clang", "clang.1", "archlinux/clang.1.gz", "CLANG(1)"),
    ("rclone", "rclone.1", "windows-releases/rclone.1.zst", "rclone(1)"),
)


def digest(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def compare(name: str, raw_name: str, fixture: str, title: str) -> dict:
    raw_file = RAW / f"{raw_name}.utf8"
    raw = visible_text(raw_file.read_text(encoding="utf-8"))
    raw_rows = raw.splitlines(keepends=True)
    if not (len(raw_rows) >= 4 and title in raw_rows[0]
            and raw_rows[1] == "\n" and raw_rows[-2] == "\n"
            and title in raw_rows[-1]):
        raise ValueError(f"{name}: patched renderer furniture is not the audited shape")
    # The known header + separator and separator + footer are the only rows
    # removed. No first/last *body* line or blank line is trimmed.
    raw_body = "".join(raw_rows[2:-2])
    unexpected_controls = sorted({ord(char) for char in raw_body
                                  if ord(char) < 32 and char != "\n"})
    if unexpected_controls:
        raise ValueError(f"{name}: normalized body retained C0 controls "
                         f"{unexpected_controls}")
    command = [str(CLI), "--annotated-preview", "--input",
               str(ROOT / "tests/fixtures/roff/real" / fixture),
               "--input-format", "roff", "--format", "text",
               "--display", "direct", "--color", "never"]
    cli = subprocess.run(command, check=True, capture_output=True, text=True,
                         timeout=60).stdout
    cli_rows = cli.splitlines(keepends=True)
    if len(cli_rows) < 2 or cli_rows[:2] != [f"{title}\n", "\n"]:
        raise ValueError(f"{name}: CLI presentation title is not the audited shape")
    cli_body = "".join(cli_rows[2:])
    if raw_body != cli_body:
        raw_lines = raw_body.splitlines(keepends=True)
        cli_lines = cli_body.splitlines(keepends=True)
        first = next((index for index, pair in enumerate(zip(raw_lines, cli_lines))
                      if pair[0] != pair[1]), min(len(raw_lines), len(cli_lines)))
        raise ValueError(f"{name}: body differs at row {first + 1}; "
                         f"raw rows={len(raw_lines)}, CLI rows={len(cli_lines)}")
    return {"page": name, "fixture": fixture, "complete": True,
            "rowsCompared": len(raw_rows) - 4,
            "patchedRawBodySha256": digest(raw_body),
            "framedCliBodySha256": digest(cli_body),
            "normalizedBodyControlsRetained": False}


def main() -> None:
    results = [compare(*fixture) for fixture in FIXTURES]
    print(json.dumps({"schema": "mant.annotated-g1-patched-geometry/v1",
                      "results": results}, indent=2))


if __name__ == "__main__":
    main()
