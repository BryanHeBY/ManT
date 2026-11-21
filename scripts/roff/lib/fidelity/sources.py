"""Select exact manual sources and their approved rendering hierarchy."""
from __future__ import annotations

import re
from pathlib import Path, PurePosixPath
from typing import Callable, Sequence

from scripts.roff.lib.roff_audit_common import (
    MANUAL_SUFFIX,
    discover_pages,
    manual_section,
    relative_label,
    source_bytes,
    source_digest,
)
from scripts.roff.lib.roff_audit_sources import (
    MAX_MANUAL_BYTES,
    possible_manual_include,
    read_verified_manual,
)

MAX_SO_REDIRECTS = 16
SO_CONTROL = rb"(?:[.']|\\\.)[ \t]*"
SO_NAME = rb"so(?:quiet)?"
SO_REQUEST = re.compile(rb"^" + SO_CONTROL + SO_NAME + rb"(?:[ \t\r]|$)", re.MULTILINE)
EXTERNAL_REQUEST = re.compile(rb"^" + SO_CONTROL + rb"m?" + SO_NAME + rb"(?:[ \t\r]|$)", re.MULTILINE)
SO_REDIRECT = re.compile(SO_CONTROL + SO_NAME + rb"[ \t]+([^ \x00-\x1f\x7f\\]+)")


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
    return not possible_manual_include(source)


def contains_so_request(path: Path) -> bool:
    """Return whether a page needs an approved hierarchy for a .so request."""
    source = source_bytes(path)
    if source is None:
        return False
    return has_so_request(source)


def has_so_request(source: bytes) -> bool:
    return SO_REQUEST.search(source.removeprefix(b"\xef\xbb\xbf")) is not None


def contains_external_request(source: bytes) -> bool:
    return EXTERNAL_REQUEST.search(source.removeprefix(b"\xef\xbb\xbf")) is not None


def redirect_only_target(source: bytes) -> str | None:
    """Return the sole relative target of a redirect-only roff source.

    With includes disabled, mandoc's read.c ROFF_SO reports a final redirect
    without opening its path. Recognize only static standalone redirects, in
    the same narrow syntax as mant-codec's mandoc/redirect.rs.
    """
    meaningful = []
    for raw_line in source.removeprefix(b"\xef\xbb\xbf").split(b"\n"):
        line = raw_line.removesuffix(b"\r")
        if b"\\#" in line or line.endswith(b"\\"):
            return None
        line = line.split(b'\\"', 1)[0].rstrip(b" \t")
        if not line or line in (b".", b"'", b"\\."):
            continue
        meaningful.append(line)
    if len(meaningful) != 1:
        return None
    match = SO_REDIRECT.fullmatch(meaningful[0])
    if match is None:
        return None
    return match.group(1).decode("utf-8", errors="surrogateescape")


def confined_redirect_target(
    path: Path,
    hierarchy_root: Path,
    target: str,
) -> Path | None:
    try:
        return resolve_redirect_target(path, hierarchy_root.resolve(strict=True), target)
    except (ValueError, OSError, RuntimeError):
        return None


def resolve_redirect_target(path: Path, hierarchy_root: Path, target: str) -> Path:
    """Use the indexed loader's root-first order and target confinement."""
    logical = PurePosixPath(target)
    if (
        logical.is_absolute()
        or not logical.parts
        or any(part in {".", ".."} for part in target.split("/"))
        or "\\" in target
    ):
        raise ValueError(f"redirect target is not a safe relative path: {target}")
    bases = (hierarchy_root, path.parent) if len(logical.parts) == 1 else (hierarchy_root,)
    for base in dict.fromkeys(bases):
        candidate = base.joinpath(*logical.parts)
        suffixes = ("",) if candidate.suffix in {".gz", ".zst"} else ("", ".gz", ".zst")
        for suffix in suffixes:
            try:
                resolved = candidate.with_name(candidate.name + suffix).resolve(strict=True)
            except FileNotFoundError:
                continue
            if not resolved.is_relative_to(hierarchy_root):
                raise ValueError(f"redirect target resolves outside the hierarchy: {target}")
            if not resolved.is_file():
                raise ValueError(f"redirect target is not a regular manual source: {target}")
            return resolved
    raise ValueError(f"redirect target is absent from the hierarchy: {target}")


def verified_source_bytes(path: Path) -> bytes | None:
    return read_verified_manual(path)[0]


def redirect_source(
    path: Path, hierarchy_root: Path, source: bytes,
    source_reader: Callable[[Path], bytes | None] = verified_source_bytes,
) -> bytes:
    """Read a bounded, confined chain and return its verified terminal source."""
    root = hierarchy_root.resolve(strict=True)
    visited = {path.resolve()}
    stored, decoded = path.stat().st_size, len(source)
    for _ in range(MAX_SO_REDIRECTS):
        target = redirect_only_target(source)
        if target is None:
            break
        path = resolve_redirect_target(path, root, target)
        if path in visited:
            raise ValueError(f"redirect cycle through {target}")
        visited.add(path)
        stored += path.stat().st_size
        if stored > MAX_MANUAL_BYTES:
            raise ValueError("redirect chain exceeds the stored manual input limit")
        next_source = source_reader(path)
        if next_source is None:
            raise ValueError(f"cannot verify redirect source transport: {target}")
        source = next_source
        decoded += len(source)
        if decoded > MAX_MANUAL_BYTES:
            raise ValueError("redirect chain exceeds the decoded manual input limit")
    if redirect_only_target(source) is not None:
        raise ValueError(f"redirect depth exceeds {MAX_SO_REDIRECTS}")
    if contains_external_request(source):
        raise ValueError("redirect chain terminates in an external include source")
    return source


def redirect_observation(path: Path, hierarchy_root: Path) -> str | None:
    """Describe dependency evidence for an audit of the alias source alone."""
    source, _ = read_verified_manual(path)
    if source is None or redirect_only_target(source) is None:
        return None
    try:
        redirect_source(path, hierarchy_root, source)
    except (ValueError, OSError, RuntimeError) as error:
        return str(error)
    return "the redirect leads to a verified source"


def mant_render_command(
    path: Path, roots: Sequence[Path], mant: Path, *, source: bytes | None = None
) -> tuple[list[str], Path | None]:
    """Select standalone or indexed rendering without weakening --input.

    Any source containing a .so request needs an approved manual hierarchy.
    Redirect-only aliases then resolve normally; unsupported embedded includes
    become visible hard failures instead of silently leaving the audit corpus.
    """
    needs_hierarchy = contains_so_request(path) if source is None else has_so_request(source)
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
        reader = source_reader or verified_source_bytes
        if source is not None and redirect_only_target(source) is not None:
            if hierarchy_root is None:
                return None, None, "redirect-only page has no owning manual hierarchy"
            try:
                reference_source = redirect_source(path, hierarchy_root, source, reader)
            except (ValueError, OSError, RuntimeError) as error:
                return None, None, str(error)
        elif source is not None and contains_external_request(source):
            return None, None, "mandoc does not expand embedded .so/.mso requests"
        else:
            reference_source = reader(path)
        if reference_source is None:
            return None, None, f"cannot decompress mandoc reference source: {path}"
        return [reference, "-T", "utf8", "-O", "width=200"], reference_source, None
    if hierarchy_root is not None:
        section = manual_section(path)
        topic = manual_topic(path)
        if section is not None and topic is not None:
            return [reference, section, topic], None, None
    return [reference, "-l", str(path)], None, None
