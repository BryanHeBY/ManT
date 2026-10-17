"""Audit finding, evidence and ledger records; no execution or file access."""
from __future__ import annotations

from dataclasses import dataclass
from .layout import LayoutComparison

@dataclass
class Finding:
    path: str
    status: str
    reference_tokens: int = 0
    mant_tokens: int = 0
    missing_tokens: list[str] | None = None
    broken_phrases: list[str] | None = None
    signatures: list[str] | None = None
    detail: str | None = None
    layout: "LayoutComparison | None" = None


@dataclass
class AuditArtifact:
    """One comparison result plus the local evidence used to review it.

    The normal JSON report remains intentionally compact and contains only
    normalized findings. An explicitly requested review bundle keeps the raw
    decompressed source and both visible renderings together so a human can
    decide whether a candidate is semantic loss or presentation drift.
    """

    finding: Finding
    source: bytes | None
    reference_output: str | None
    mant_output: str | None


@dataclass
class AuditSummary:
    examined: int = 0
    clean: int = 0
    review: int = 0
    hard_failures: int = 0
    skipped: int = 0


@dataclass(frozen=True)
class AuditRecord:
    corpus: str
    path: str
    section: str
    digest: str
    scan_status: str
    review_status: str
    note: str
    reference_kind: str = "man"
    reference_id: str = ""


@dataclass(frozen=True)
class SyntaxProfile:
    features: frozenset[str]
    diagnostics: int
    error: str | None = None
