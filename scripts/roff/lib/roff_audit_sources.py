"""Discover manual leaves and establish identities for roff audit inputs.

Source readers that collect evidence can decode a broader set of containers
than the file-based audit backends accept. Only a verified read can authorize
reusing an observation bound to a decoded source digest.
"""

from __future__ import annotations

import bz2
import gzip
import hashlib
import lzma
import os
import re
import shutil
import subprocess
import tempfile
import zlib
from pathlib import Path
from typing import BinaryIO, Sequence


MANUAL_SUFFIX = re.compile(
    r"\.(?P<section>[0-9][0-9A-Za-z]*|[ln])(?:\.(?:gz|bz2|xz|zst))?$"
)

MANUAL_COMPRESSION = {".gz": b"\x1f\x8b", ".zst": b"\x28\xb5\x2f\xfd",
                      ".xz": b"\xfd7zXZ\x00", ".bz2": b"BZh"}
MAX_MANUAL_BYTES = 16 * 1024 * 1024
MANUAL_INCLUDE = re.compile(rb"(?:^|[\s\\])[.'][ \t]*m?so(?:[ \t]|$)")


def discover_pages(roots: Sequence[Path], *, strict: bool = False) -> list[Path]:
    def report(error: OSError) -> None:
        if strict:
            raise ValueError(f"could not enumerate manual root: {error}") from error

    pages: set[Path] = set()
    for root in roots:
        if not root.is_dir():
            raise ValueError(f"manual root is not a directory: {root}")
        paths = (Path(folder) / name for folder, dirs, files in os.walk(root, onerror=report)
                 for name in (*dirs, *files)) if strict else root.rglob("*")
        for path in paths:
            if (path.is_file() or path.is_symlink()) and MANUAL_SUFFIX.search(path.name):
                pages.add(path)
    return sorted(pages, key=lambda path: path.as_posix())


def manual_section(path: Path) -> str | None:
    match = MANUAL_SUFFIX.search(path.name)
    return match.group("section") if match is not None else None


def relative_label(path: Path, roots: Sequence[Path]) -> str:
    common = Path(os.path.commonpath(roots)) if len(roots) > 1 else None
    for root in roots:
        try:
            relative = path.relative_to(root)
        except ValueError:
            continue
        prefix = root.relative_to(common) if common is not None else Path(root.name)
        return (prefix / relative).as_posix()
    return path.as_posix()


def manual_hierarchy_root(path: Path, roots: Sequence[Path]) -> Path | None:
    """Return the narrow hierarchy owning one exact leaf and its `.so` targets."""
    section = manual_section(path)
    if section is None:
        return None
    for root in roots:
        try:
            relative = path.relative_to(root)
        except ValueError:
            continue
        candidates = [
            index
            for index, part in enumerate(relative.parts[:-1])
            if part.startswith("man") and part[3:] and section.startswith(part[3:])
        ]
        return root.joinpath(*relative.parts[: candidates[-1]]) if candidates else root
    return None


def decode_source(path: Path) -> bytes:
    """Decode one page or report the decompressor's page-local failure."""
    if path.name.endswith(".gz"):
        with gzip.open(path, "rb") as stream:
            return stream.read()
    if path.name.endswith(".bz2"):
        with bz2.open(path, "rb") as stream:
            return stream.read()
    if path.name.endswith(".xz"):
        with lzma.open(path, "rb") as stream:
            return stream.read()
    if path.name.endswith(".zst"):
        zstd = shutil.which("zstd")
        if zstd is None:
            raise OSError("zstd decompressor not found")
        result = subprocess.run(
            [zstd, "--decompress", "--stdout", str(path)], capture_output=True, check=False,
        )
        if result.returncode != 0:
            raise OSError(result.stderr.decode("utf-8", errors="replace").strip()
                          or f"zstd exited with status {result.returncode}")
        return result.stdout
    return path.read_bytes()


def read_source(path: Path) -> tuple[bytes | None, str | None]:
    try:
        return decode_source(path), None
    except (OSError, EOFError, lzma.LZMAError, zlib.error) as error:
        return None, " ".join(str(error).splitlines()) or type(error).__name__


def read_verified_manual(path: Path) -> tuple[bytes | None, str | None]:
    """Accept only transport identities shared by the file-based audit backends."""
    try:
        with path.open("rb", buffering=0) as stream:
            header = stream.read(6)
            detected = manual_compression(header)
            if error := manual_transport_error(path, detected):
                return None, error
            if os.fstat(stream.fileno()).st_size > MAX_MANUAL_BYTES:
                return None, "stored manual exceeds the loader input limit"
            if detected is None:
                return header + stream.read(), None
            stream.seek(0)
            if detected == ".gz":
                return decode_verified_gzip(stream), None
            return decode_verified_zstd(stream), None
    except (OSError, EOFError, zlib.error) as error:
        return None, " ".join(str(error).splitlines()) or type(error).__name__

def decode_verified_gzip(stream: BinaryIO) -> bytes:
    # MultiGzDecoder requires each group of bytes after a member to start
    # another member. GzipFile would silently consume a zero-padded tail.
    member = zlib.decompressobj(31)
    decoded = bytearray()
    while chunk := stream.read(64 * 1024):
        while chunk:
            if member.eof:
                member = zlib.decompressobj(31)
            decoded.extend(member.decompress(chunk, MAX_MANUAL_BYTES + 1 - len(decoded)))
            if len(decoded) > MAX_MANUAL_BYTES:
                raise OSError("decoded manual exceeds the loader input limit")
            chunk = member.unused_data
    if not member.eof:
        raise EOFError("incomplete gzip member")
    return bytes(decoded)


def decode_verified_zstd(stream: BinaryIO) -> bytes:
    zstd = shutil.which("zstd")
    if zstd is None:
        raise OSError("zstd decompressor not found")
    with tempfile.TemporaryFile() as errors, subprocess.Popen(
        [zstd, "--decompress", "--stdout"], stdin=stream, stdout=subprocess.PIPE, stderr=errors,
    ) as decoder:
        assert decoder.stdout is not None
        source = decoder.stdout.read(MAX_MANUAL_BYTES + 1)
        if len(source) > MAX_MANUAL_BYTES:
            decoder.kill()
            raise OSError("decoded manual exceeds the loader input limit")
        if decoder.wait() != 0:
            errors.seek(0)
            reason = errors.read(4096).decode("utf-8", errors="replace").strip()
            raise OSError(reason or "zstd failed to decode the source")
        return source


def manual_compression(header: bytes) -> str | None:
    if len(header) >= 4 and 0x50 <= header[0] <= 0x5f and header[1:4] == b"\x2a\x4d\x18":
        return ".zst"
    return next((suffix for suffix, magic in MANUAL_COMPRESSION.items() if header.startswith(magic)), None)


def manual_transport_error(path: Path, detected: str | None) -> str | None:
    suffix = path.suffix if path.suffix in MANUAL_COMPRESSION else None
    if detected != suffix:
        return "compression header does not match the manual's suffix"
    if detected in {".xz", ".bz2"}:
        return f"the audit backends cannot verify {detected} input"
    return None


def observe_sources(
    pages: Sequence[Path], roots: Sequence[Path], patterns: Sequence[re.Pattern[str]],
    *, refresh: set[Path] | None = None,
) -> tuple[dict[Path, tuple[str, str | None]], dict[Path, str]]:
    records, failures = {}, {}
    for path in pages:
        source, error = read_verified_manual(path)
        label = relative_label(path, roots)
        if source is None:
            failures[path] = error or "source is unavailable"
            if not patterns:
                records[path] = label, None
            continue
        if not patterns or all(pattern.search(source.decode("utf-8", errors="replace")) for pattern in patterns):
            records[path] = label, hashlib.sha256(source).hexdigest()
            if refresh is not None and (path.is_symlink() or MANUAL_INCLUDE.search(source)):
                # The entry digest says nothing about the current include
                # target. Until dependencies are recorded, observe it again.
                refresh.add(path)
    return records, failures


def verify_source_digest(path: Path, digest: str) -> bool:
    source, _ = read_verified_manual(path)
    return source is not None and hashlib.sha256(source).hexdigest() == digest


def uncovered_sources(failures: dict[Path, str], roots: Sequence[Path]) -> list[dict[str, str]]:
    return [{"path": relative_label(path, roots), "reason": reason} for path, reason in failures.items()]


def source_bytes(path: Path) -> bytes | None:
    return read_source(path)[0]


def source_digest(path: Path) -> str | None:
    source = source_bytes(path)
    return hashlib.sha256(source).hexdigest() if source is not None else None


def compile_source_patterns(values: Sequence[str] | None) -> list[re.Pattern[str]]:
    patterns = []
    for value in values or []:
        try:
            patterns.append(re.compile(value, re.MULTILINE))
        except re.error as error:
            raise ValueError(f"invalid --source-pattern {value!r}: {error}") from error
    return patterns


def filter_pages_by_source(
    pages: Sequence[Path], patterns: Sequence[re.Pattern[str]]
) -> tuple[list[Path], list[Path]]:
    if not patterns:
        return list(pages), []
    selected = []
    unreadable = []
    for path in pages:
        source = source_bytes(path)
        if source is None:
            unreadable.append(path)
        elif all(pattern.search(source.decode("utf-8", errors="replace")) for pattern in patterns):
            selected.append(path)
    return selected, unreadable
