"""Select exact manual sources and their approved rendering hierarchy."""
from __future__ import annotations

import re
from pathlib import Path, PurePosixPath
from typing import Sequence

from scripts.roff.lib.roff_audit_common import (
    MANUAL_SUFFIX,
    discover_pages,
    manual_section,
    relative_label,
    source_bytes,
    source_digest,
)
from .text import EXTERNAL_ROFF_CONTEXT


def explicit_pages(path: Path, roots: Sequence[Path]) -> list[Path]:
    """Read an exact audit set without loosening the selected manual roots.

    A caller that already selected pages (for example the structure audit) must
    not accidentally re-run a different sample because its own CSV completion
    state differs from the fidelity ledger.  Paths are deliberately absolute
    and must lexically remain below one supplied root; this keeps labels and
    `.so` hierarchy resolution identical to normal discovery.
    """
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise ValueError(f"cannot read --pages-file {path}: {error}") from error
    pages: set[Path] = set()
    for number, value in enumerate(lines, 1):
        if not value:
            continue
        candidate = Path(value)
        if not candidate.is_absolute():
            raise ValueError(f"--pages-file {path}:{number} is not an absolute path")
        if not MANUAL_SUFFIX.search(candidate.name):
            raise ValueError(f"--pages-file {path}:{number} is not a manual page")
        if not any(candidate.is_relative_to(root) for root in roots):
            raise ValueError(
                f"--pages-file {path}:{number} is outside the selected manual roots"
            )
        if not (candidate.is_file() or candidate.is_symlink()):
            raise ValueError(f"--pages-file {path}:{number} is not a readable manual page")
        pages.add(candidate)
    if not pages:
        raise ValueError(f"--pages-file {path} did not select any manual pages")
    return sorted(pages, key=lambda candidate: candidate.as_posix())


def manual_topic(path: Path) -> str | None:
    match = MANUAL_SUFFIX.search(path.name)
    return path.name[: match.start()] if match is not None else None


def manual_hierarchy_root(path: Path, roots: Sequence[Path]) -> Path | None:
    """Return the narrow hierarchy root owning an exact manual leaf.

    A localized page such as ``ROOT/fr/man1/demo.1`` belongs to ``ROOT/fr``.
    Using that root with MANT_MANPATH selects the exact localized leaf while
    keeping ``.so man1/...`` redirects inside the same approved hierarchy.
    """
    section = manual_section(path)
    if section is None:
        return None
    for root in roots:
        try:
            relative = path.relative_to(root)
        except ValueError:
            continue
        parts = relative.parts
        candidates = [
            index
            for index, part in enumerate(parts[:-1])
            if part.startswith("man")
            and part[3:]
            and section.startswith(part[3:])
        ]
        if not candidates:
            return root
        index = candidates[-1]
        return root.joinpath(*parts[:index])
    return None


def filter_pages_by_source(
    pages: Sequence[Path], patterns: Sequence[re.Pattern[str]]
) -> tuple[list[Path], list[Path]]:
    if not patterns:
        return list(pages), []
    selected = []
    unreadable = []
    for path in pages:
        raw = source_bytes(path)
        if raw is None:
            unreadable.append(path)
            continue
        source = raw.decode("utf-8", errors="replace")
        if all(pattern.search(source) for pattern in patterns):
            selected.append(path)
    return selected, unreadable


def source_audit_identity(label: str, digest: str) -> tuple[str, str, str] | None:
    path = Path(label)
    topic = manual_topic(path)
    section = manual_section(path)
    if topic is None or section is None:
        return None
    return digest, topic, section


def source_is_context_independent(path: Path) -> bool:
    source = source_bytes(path)
    return source is not None and source_bytes_are_context_independent(source)


def source_bytes_are_context_independent(source: bytes) -> bool:
    return EXTERNAL_ROFF_CONTEXT.search(source) is None


def contains_so_request(path: Path) -> bool:
    """Return whether a page needs an approved hierarchy for a .so request."""
    source = source_bytes(path)
    if source is None:
        return False
    return any(
        re.match(rb"^[.']so(?:[ \t]|$)", line) is not None
        for line in source.splitlines()
    )


def contains_external_request(source: bytes) -> bool:
    return any(
        re.match(rb"^[.'](?:so|mso)(?:[ \t]|$)", line) is not None
        for line in source.splitlines()
    )


def redirect_only_target(source: bytes) -> str | None:
    """Return the sole relative target of a redirect-only roff source.

    mandoc intentionally reports a top-level ``.so`` request instead of
    following it. Resolve only the unambiguous alias shape used by manual
    hierarchies; embedded includes remain non-comparable rather than growing a
    second roff interpreter in this audit script.
    """
    meaningful = []
    for raw_line in source.splitlines():
        line = raw_line.strip()
        if not line or line.startswith((b'.\\"', b"'\\\"")):
            continue
        meaningful.append(line)
    if len(meaningful) != 1:
        return None
    match = re.fullmatch(rb"[.']so[ \t]+([^ \t\x00]+)[ \t]*", meaningful[0])
    if match is None:
        return None
    return match.group(1).decode("utf-8", errors="surrogateescape")


def confined_redirect_target(
    path: Path,
    hierarchy_root: Path,
    target: str,
) -> Path | None:
    """Resolve one mandoc alias target inside its owning manual hierarchy."""
    logical = PurePosixPath(target)
    if (
        logical.is_absolute()
        or not logical.parts
        or any(part in {"", ".", ".."} for part in logical.parts)
        or "\\" in target
    ):
        return None
    base = hierarchy_root if logical.parts[0].startswith("man") else path.parent
    candidate = base.joinpath(*logical.parts)
    try:
        candidate.relative_to(hierarchy_root)
    except ValueError:
        return None
    candidates = [candidate]
    if not candidate.name.endswith((".gz", ".bz2", ".xz", ".zst")):
        candidates.extend(
            candidate.with_name(f"{candidate.name}{suffix}")
            for suffix in (".gz", ".bz2", ".xz", ".zst")
        )
    return next(
        (value for value in candidates if value.is_file() or value.is_symlink()),
        None,
    )


def mant_render_command(
    path: Path, roots: Sequence[Path], mant: Path, *, source: bytes | None = None
) -> tuple[list[str], Path | None]:
    """Select standalone or indexed rendering without weakening --input.

    Any source containing a .so request needs an approved manual hierarchy.
    Redirect-only aliases then resolve normally; unsupported embedded includes
    become visible hard failures instead of silently leaving the audit corpus.
    """
    needs_hierarchy = (contains_so_request(path) if source is None else
        any(re.match(rb"^[.']so(?:[ \t]|$)", line) is not None for line in source.splitlines()))
    if needs_hierarchy:
        section = manual_section(path)
        topic = manual_topic(path)
        root = manual_hierarchy_root(path, roots)
        if section is not None and topic is not None and root is not None:
            return (
                [
                    str(mant),
                    f"manual/{section}/{topic}",
                    "--manual",
                    "--format",
                    "man",
                ],
                root,
            )
    return (
        [
            str(mant),
            "--input",
            str(path),
            "--input-format",
            "roff",
            "--format",
            "man",
        ],
        None,
    )


def reference_render_command(
    path: Path,
    reference: str,
    reference_kind: str,
    hierarchy_root: Path | None,
    source: bytes | None,
    *, source_reader=None,
) -> tuple[list[str] | None, bytes | None, str | None]:
    """Build a renderer invocation or explain why the source is not comparable."""
    if reference_kind == "mandoc":
        reference_path = path
        if source is not None and contains_external_request(source):
            target = redirect_only_target(source)
            if target is None:
                return None, None, "mandoc does not expand embedded .so/.mso requests"
            if hierarchy_root is None:
                return None, None, "redirect-only page has no owning manual hierarchy"
            resolved = confined_redirect_target(path, hierarchy_root, target)
            if resolved is None:
                return None, None, f"redirect target is absent or outside the hierarchy: {target}"
            reference_path = resolved
        reference_source = (source_reader or source_bytes)(reference_path)
        if reference_source is None:
            return None, None, f"cannot decompress mandoc reference source: {reference_path}"
        return [reference, "-T", "utf8", "-O", "width=200"], reference_source, None
    if hierarchy_root is not None:
        section = manual_section(path)
        topic = manual_topic(path)
        if section is not None and topic is not None:
            return [reference, section, topic], None, None
    return [reference, "-l", str(path)], None, None
