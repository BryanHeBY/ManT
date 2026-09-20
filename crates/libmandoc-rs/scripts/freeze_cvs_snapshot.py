#!/usr/bin/env python3
"""Freeze one official mandoc CVS cutoff without modifying the active vendor.

The command performs two independent checkouts at one explicit UTC cutoff,
derives revisions from CVS/Entries, compares both trees, and emits a candidate
source lock plus a deterministic archive.  All staging and output paths must
live below the repository target directory.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

import sync_vendor


CVSROOT = ":ext:anoncvs@mandoc.bsd.lv:/cvs"
MODULE = "mandoc"
CUTOFF_FORMAT = "%Y-%m-%d %H:%M:%S UTC"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def parse_cutoff(value: str) -> datetime:
    if value == "now":
        return datetime.now(timezone.utc).replace(microsecond=0)
    parsed = datetime.strptime(value, CUTOFF_FORMAT)
    return parsed.replace(tzinfo=timezone.utc)


def cutoff_text(value: datetime) -> str:
    return value.astimezone(timezone.utc).strftime(CUTOFF_FORMAT)


def checked_target_path(repository: Path, value: Path, label: str) -> Path:
    repository = repository.resolve()
    target = (repository / "target").resolve()
    path = value.resolve()
    if not path.is_relative_to(target) or path == target:
        raise ValueError(f"{label} must be a dedicated path below {target}")
    return path


def cvs_version(cvs: str) -> str:
    result = subprocess.run(
        [cvs, "--version"], check=True, text=True, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    return result.stdout.strip()


def checkout(cvs: str, wrapper: Path, cutoff: datetime, destination: Path) -> Path:
    environment = dict(os.environ, CVS_RSH=str(wrapper))
    subprocess.run(
        [cvs, "-Q", "-d", CVSROOT, "checkout", "-D", cutoff_text(cutoff),
         "-d", destination.name, MODULE],
        cwd=destination.parent, env=environment, check=True,
    )
    if not destination.is_dir() or destination.is_symlink():
        raise ValueError("CVS did not create a regular checkout directory")
    return destination


def _entries(directory: Path) -> tuple[dict[str, str], set[str]]:
    path = directory / "CVS" / "Entries"
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"missing CVS/Entries: {directory}")
    files: dict[str, str] = {}
    subdirectories: set[str] = set()
    for number, line in enumerate(path.read_text(errors="strict").splitlines(), 1):
        if not line:
            continue
        if line == "D":
            continue
        if line.startswith("D/"):
            fields = line.split("/")
            if len(fields) < 2 or not fields[1]:
                raise ValueError(f"invalid CVS directory entry at {path}:{number}")
            sync_vendor.portable_path(fields[1])
            subdirectories.add(fields[1])
            continue
        if not line.startswith("/"):
            raise ValueError(f"unsupported CVS entry at {path}:{number}")
        fields = line.split("/")
        if len(fields) < 4 or not fields[1] or not fields[2]:
            raise ValueError(f"invalid CVS file entry at {path}:{number}")
        name, revision, timestamp = fields[1], fields[2], fields[3]
        if (PurePosixPath(name).name != name
                or not re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", revision)
                or revision.startswith("0") or timestamp.startswith("Result of merge")):
            raise ValueError(f"non-pristine CVS entry at {path}:{number}")
        sync_vendor.portable_path(name)
        if name in files:
            raise ValueError(f"duplicate CVS entry at {path}:{number}")
        files[name] = revision
    return files, subdirectories


def inventory(checkout_root: Path) -> dict[str, dict[str, str]]:
    """Return every checked-out file and reject metadata/tree inconsistencies."""
    if checkout_root.is_symlink() or not checkout_root.is_dir():
        raise ValueError("checkout root must be a regular directory")
    result: dict[str, dict[str, str]] = {}
    spellings: dict[str, str] = {}
    pending = [checkout_root]
    while pending:
        directory = pending.pop()
        relative_directory = directory.relative_to(checkout_root)
        expected_files, expected_directories = _entries(directory)
        actual_files: set[str] = set()
        actual_directories: set[str] = set()
        for path in directory.iterdir():
            if path.name == "CVS":
                if not path.is_dir() or path.is_symlink():
                    raise ValueError(f"invalid CVS metadata path: {path}")
                continue
            relative = path.relative_to(checkout_root)
            portable = sync_vendor.portable_path(relative.as_posix())
            sync_vendor.register_portable_path(portable, spellings)
            if path.is_symlink() or not (path.is_file() or path.is_dir()):
                raise ValueError(f"nonregular checkout path: {relative}")
            if path.is_dir():
                actual_directories.add(path.name)
                pending.append(path)
            else:
                actual_files.add(path.name)
                revision = expected_files.get(path.name)
                if revision is None:
                    raise ValueError(f"file missing from CVS/Entries: {relative}")
                name = relative.as_posix()
                result[name] = {
                    "revision": revision,
                    "sha256": sha256(path),
                    "scope": "regress" if relative.parts[0] == "regress" else "shipping",
                }
        if actual_files != set(expected_files):
            raise ValueError(
                f"CVS file inventory mismatch in {relative_directory}: "
                f"missing={sorted(set(expected_files) - actual_files)}, "
                f"extra={sorted(actual_files - set(expected_files))}"
            )
        # Some CVS clients omit D lines when Entries.Log is in use.  Actual
        # directories remain accepted only when they carry their own Entries.
        unexpected = actual_directories - expected_directories
        for name in unexpected:
            if not (directory / name / "CVS" / "Entries").is_file():
                raise ValueError(f"directory missing from CVS metadata: {relative_directory / name}")
        missing = expected_directories - actual_directories
        if missing:
            raise ValueError(f"CVS directories missing from checkout: {sorted(missing)}")
    if not result:
        raise ValueError("CVS checkout contains no source files")
    return dict(sorted(result.items()))


def shipping_manifest(entries: dict[str, dict[str, str]], cutoff: datetime) -> str:
    rows = [f"# Pinned CVS {cutoff_text(cutoff)}; SHA-256, revision, source path.\n"]
    for name, entry in entries.items():
        if entry["scope"] == "shipping":
            rows.append(f"{entry['sha256']}\t{entry['revision']}\t{name}\n")
    if len(rows) == 1:
        raise ValueError("shipping source inventory is empty")
    return "".join(rows)


def source_lock(
    version: str,
    cutoff: datetime,
    manifest_hash: str,
    archive: str,
    archive_hash: str,
    inventory_hash: str,
    regress_hash: str,
) -> str:
    return (
        "# Fixed upstream mandoc CVS snapshot used as the vendor base.\n"
        "# Generated by scripts/freeze-cvs-snapshot; ordinary builds never fetch it.\n\n"
        "kind = cvs\n"
        f"version = {version}\n"
        f"cvsroot = {CVSROOT}\n"
        f"module = {MODULE}\n"
        f"date = {cutoff_text(cutoff)}\n"
        "root = mandoc\n"
        "manifest = FILES\n"
        f"manifest_sha256 = {manifest_hash}\n"
        f"archive = {archive}\n"
        f"archive_sha256 = {archive_hash}\n"
        "inventory = CVS_INVENTORY.json\n"
        f"inventory_sha256 = {inventory_hash}\n"
        "regress_manifest = REGRESS_FILES\n"
        f"regress_manifest_sha256 = {regress_hash}\n"
    )


def write_archive(checkout_root: Path, entries: dict[str, dict[str, str]], output: Path) -> None:
    with output.open("xb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                root = tarfile.TarInfo("mandoc")
                root.type = tarfile.DIRTYPE
                root.mode, root.uid, root.gid, root.mtime = 0o755, 0, 0, 0
                archive.addfile(root)
                directories: set[PurePosixPath] = set()
                for name in entries:
                    relative = PurePosixPath(name)
                    directories.update(parent for parent in relative.parents if parent != PurePosixPath("."))
                for directory in sorted(directories, key=lambda item: (len(item.parts), item.as_posix())):
                    info = tarfile.TarInfo(f"mandoc/{directory.as_posix()}")
                    info.type = tarfile.DIRTYPE
                    info.mode, info.uid, info.gid, info.mtime = 0o755, 0, 0, 0
                    archive.addfile(info)
                for name in entries:
                    path = checkout_root.joinpath(*PurePosixPath(name).parts)
                    info = tarfile.TarInfo(f"mandoc/{name}")
                    info.size = path.stat().st_size
                    info.mode = 0o755 if path.stat().st_mode & 0o111 else 0o644
                    info.uid, info.gid, info.mtime = 0, 0, 0
                    with path.open("rb") as source:
                        archive.addfile(info, source)


def inventory_diff(old_manifest: Path, current: dict[str, dict[str, str]]) -> dict[str, list[str]]:
    old: dict[str, tuple[str, str]] = {}
    if old_manifest.is_file():
        for line in old_manifest.read_text().splitlines():
            if line and not line.startswith("#"):
                checksum, revision, name = line.split("\t")
                old[name] = (checksum, revision)
    shipping = {name: entry for name, entry in current.items() if entry["scope"] == "shipping"}
    common = old.keys() & shipping.keys()
    return {
        "added": sorted(shipping.keys() - old.keys()),
        "removed": sorted(old.keys() - shipping.keys()),
        "revisionChanged": sorted(name for name in common if old[name][1] != shipping[name]["revision"]),
        "contentChanged": sorted(name for name in common if old[name][0] != shipping[name]["sha256"]),
    }


def freeze(repository: Path, cvs: str, cutoff: datetime, work: Path, output: Path) -> dict:
    repository = repository.resolve()
    work = checked_target_path(repository, work, "work directory")
    output = checked_target_path(repository, output, "output directory")
    if work.exists() or output.exists():
        raise ValueError("work and output directories must not already exist")
    if work == output or work.is_relative_to(output) or output.is_relative_to(work):
        raise ValueError("work and output directories must be disjoint")
    work.mkdir(parents=True)
    wrapper = repository / "crates/libmandoc-rs/scripts/cvs-ssh"
    first = checkout(cvs, wrapper, cutoff, work / "checkout-a")
    second = checkout(cvs, wrapper, cutoff, work / "checkout-b")
    first_inventory, second_inventory = inventory(first), inventory(second)
    if first_inventory != second_inventory:
        raise ValueError("independent CVS checkouts disagree at the selected cutoff")

    version = "cvs-" + cutoff.strftime("%Y%m%dT%H%M%SZ")
    cvs_path = Path(cvs).resolve()
    manifest_text = shipping_manifest(first_inventory, cutoff)
    with tempfile.TemporaryDirectory(prefix=".freeze-candidate-", dir=output.parent) as temporary:
        candidate = Path(temporary) / "candidate"
        candidate.mkdir()
        manifest = candidate / "FILES"
        manifest.write_text(manifest_text)
        regress = candidate / "REGRESS_FILES"
        regress.write_text("".join(
            f"{entry['sha256']}\t{entry['revision']}\t{name}\n"
            for name, entry in first_inventory.items() if entry["scope"] == "regress"
        ))
        complete = candidate / "CVS_INVENTORY.json"
        complete.write_text(json.dumps({
            "schema": "libmandoc-rs.cvs-inventory/v1",
            "cutoff": cutoff_text(cutoff),
            "shippingCount": sum(x["scope"] == "shipping" for x in first_inventory.values()),
            "regressCount": sum(x["scope"] == "regress" for x in first_inventory.values()),
            "files": first_inventory,
        }, sort_keys=True, indent=2) + "\n")
        archive = candidate / f"upstream-{version}.tar.gz"
        write_archive(first, first_inventory, archive)
        lock = candidate / "SOURCE"
        lock.write_text(source_lock(
            version, cutoff, sha256(manifest), archive.name, sha256(archive),
            sha256(complete), sha256(regress),
        ))
        report = {
            "schema": "libmandoc-rs.cvs-freeze/v1",
            "version": version,
            "cutoff": cutoff_text(cutoff),
            "cvsClient": cvs_version(cvs),
            "cvsExecutable": {"path": str(cvs_path), "sha256": sha256(cvs_path)},
            "checkoutCommand": [cvs, "-Q", "-d", CVSROOT, "checkout", "-D", cutoff_text(cutoff), "-d", "<destination>", MODULE],
            "sourceSha256": sha256(lock),
            "manifestSha256": sha256(manifest),
            "regressManifestSha256": sha256(regress),
            "inventorySha256": sha256(complete),
            "archive": archive.name,
            "archiveSha256": sha256(archive),
            "inventoryDiff": inventory_diff(repository / "crates/libmandoc-rs/upstream/FILES", first_inventory),
        }
        (candidate / "SNAPSHOT_REPORT.json").write_text(
            json.dumps(report, sort_keys=True, indent=2) + "\n"
        )
        # Prove that the emitted archive reproduces the locked shipping tree.
        with tempfile.TemporaryDirectory(prefix="archive-check-", dir=work) as verification:
            unpacked = sync_vendor.extract_archive(archive, Path(verification), "mandoc")
            entries = sync_vendor.read_manifest(manifest, report["manifestSha256"])
            sync_vendor.verify_cvs_tree(unpacked, entries)
        candidate.rename(output)
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cutoff", required=True,
                        help=f"explicit UTC cutoff ({CUTOFF_FORMAT.replace('%', '%%')})")
    parser.add_argument("--cvs", default=os.environ.get("CVS", "cvs"), help="CVS client executable")
    parser.add_argument("--work", type=Path, required=True, help="new diagnostic staging directory below target")
    parser.add_argument("--output", type=Path, required=True, help="new candidate directory below target")
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[3], help=argparse.SUPPRESS)
    args = parser.parse_args()
    cutoff = parse_cutoff(args.cutoff)
    try:
        report = freeze(args.repository, args.cvs, cutoff, args.work, args.output)
    except (OSError, ValueError, subprocess.CalledProcessError, tarfile.TarError) as error:
        try:
            work = checked_target_path(args.repository, args.work, "work directory")
            work.mkdir(parents=True, exist_ok=True)
            (work / "failure.json").write_text(json.dumps({
                "schema": "libmandoc-rs.cvs-freeze-failure/v1",
                "cutoff": cutoff_text(cutoff),
                "error": f"{type(error).__name__}: {error}",
            }, indent=2) + "\n")
        except (OSError, ValueError):
            pass
        print(f"freeze-cvs-snapshot: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, sort_keys=True, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
