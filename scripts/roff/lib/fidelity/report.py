"""Explicit fidelity report and review-bundle output; never writes audit ledgers."""
from __future__ import annotations

import hashlib
import json
import re
from dataclasses import asdict
from pathlib import Path
from typing import Sequence
from .model import AuditArtifact, AuditSummary, Finding

def print_finding(finding: Finding, show: int) -> None:
    badge = {
        "clean": "CLEAN",
        "review": "REVIEW",
        "hard-failure": "HARD",
        "skipped": "SKIP",
    }[finding.status]
    print(
        f"{badge:6} {finding.path} "
        f"[tokens: reference={finding.reference_tokens}, mant={finding.mant_tokens}]"
    )
    if finding.detail:
        print(f"       detail: {finding.detail}")
    if finding.signatures:
        for signature in finding.signatures:
            print(f"       signature: {signature}")
    if finding.missing_tokens:
        values = ", ".join(finding.missing_tokens[:show])
        suffix = " …" if len(finding.missing_tokens) > show else ""
        print(f"       missing tokens ({len(finding.missing_tokens)}): {values}{suffix}")
    if finding.broken_phrases:
        print(f"       broken phrases ({len(finding.broken_phrases)}):")
        for phrase in finding.broken_phrases[:show]:
            print(f"         - {phrase}")
        if len(finding.broken_phrases) > show:
            print("         - …")
    if finding.layout and finding.layout.candidates:
        print(f"       layout anchors: {finding.layout.shared_anchors}")
        for candidate in finding.layout.candidates:
            print(f"       layout: {candidate}")


def update_summary(summary: AuditSummary, finding: Finding) -> None:
    summary.examined += 1
    if finding.status == "clean":
        summary.clean += 1
    elif finding.status == "review":
        summary.review += 1
    elif finding.status == "hard-failure":
        summary.hard_failures += 1
    else:
        summary.skipped += 1


def write_json_report(
    path: Path,
    roots: Sequence[Path],
    mant: Path,
    reference: str,
    reference_kind: str,
    reference_id: str,
    layout_signals: bool,
    summary: AuditSummary,
    findings: Sequence[Finding],
) -> None:
    report = {
        "tool": "mant-roff-fidelity-audit/v1",
        "roots": [str(root) for root in roots],
        "mant": str(mant),
        "reference": reference,
        "referenceKind": reference_kind,
        "referenceId": reference_id,
        "layout_signals": layout_signals,
        "summary": asdict(summary),
        "findings": [asdict(finding) for finding in findings],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def review_bundle_name(label: str) -> str:
    """Return one stable, path-safe directory name for a reviewed page."""
    leaf = re.sub(r"[^A-Za-z0-9._-]+", "-", Path(label).name).strip(".-")
    readable = leaf or "manual"
    digest = hashlib.sha256(label.encode("utf-8")).hexdigest()[:16]
    return f"{readable}-{digest}"


def write_review_bundle(
    directory: Path,
    artifacts: Sequence[tuple[str, AuditArtifact]],
) -> None:
    """Write local source/render evidence without changing the audit ledger.

    The bundle is intentionally opt-in and is not a fixture format: it may
    contain third-party manuals and renderer-specific text.  A stable hash of
    the logical path avoids interpreting a source label as a filesystem path.
    """
    pages = directory / "pages"
    pages.mkdir(parents=True, exist_ok=True)
    entries = []
    for label, artifact in sorted(artifacts, key=lambda value: value[0]):
        name = review_bundle_name(label)
        page_directory = pages / name
        page_directory.mkdir(parents=True, exist_ok=True)
        files: dict[str, str] = {}
        if artifact.source is not None:
            (page_directory / "source.roff").write_bytes(artifact.source)
            files["source"] = f"pages/{name}/source.roff"
        if artifact.reference_output is not None:
            (page_directory / "reference.txt").write_text(
                artifact.reference_output,
                encoding="utf-8",
            )
            files["reference"] = f"pages/{name}/reference.txt"
        if artifact.mant_output is not None:
            (page_directory / "mant.txt").write_text(
                artifact.mant_output,
                encoding="utf-8",
            )
            files["mant"] = f"pages/{name}/mant.txt"
        (page_directory / "finding.json").write_text(
            json.dumps(asdict(artifact.finding), indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        files["finding"] = f"pages/{name}/finding.json"
        entries.append(
            {
                "path": label,
                "status": artifact.finding.status,
                "files": files,
            }
        )
    manifest = {
        "tool": "mant-roff-fidelity-review/v1",
        "pages": entries,
    }
    (directory / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
