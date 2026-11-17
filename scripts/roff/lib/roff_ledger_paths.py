"""Keep reproducible fixture ledgers separate from large corpus audit runs."""
from __future__ import annotations

import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


def audit_ledger_path(explicit: Path | None, fixture_default: Path, *, external: bool) -> Path:
    """Explicit paths win; external scans never default to tracked fixture data.

    MANT_ROFF_CORPUS selects a separate corpus checkout. Historical imports are
    immutable under audits/mant-tracked; fresh runs use audits/mant/current.
    Without a configured corpus checkout, external runs remain untracked in
    ManT's target directory. This function performs no filesystem writes.
    """
    if explicit is not None:
        return explicit
    if not external:
        return fixture_default
    configured = os.environ.get("MANT_ROFF_CORPUS", "").strip()
    directory = (Path(configured).expanduser() / "audits/mant/current" if configured
                 else ROOT / "target/roff-audits")
    return directory / fixture_default.name
