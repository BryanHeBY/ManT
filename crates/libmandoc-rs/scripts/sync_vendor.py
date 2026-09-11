#!/usr/bin/env python3
"""Replay one locked release or CVS source tree through one patch series."""

import argparse
import datetime
import hashlib
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def component(value):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", value) or value in {".", ".."}:
        raise ValueError(f"invalid source component: {value!r}")
    return value


def read_source(path):
    fields = {}
    for number, line in enumerate(path.read_text().splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, value = line.partition("=")
        key, value = key.strip(), value.strip()
        if not separator or not value or key in fields:
            raise ValueError(f"invalid/duplicate SOURCE field at line {number}")
        fields[key] = value
    shared = {"kind", "version"}
    kinds = {
        "release": {"url", "sha256"},
        "cvs": {"cvsroot", "module", "date", "root", "manifest", "manifest_sha256"},
    }
    kind = fields.get("kind")
    if kind not in kinds or fields.keys() != shared | kinds[kind]:
        raise ValueError("SOURCE has missing, unknown, or incompatible fields")
    component(fields["version"])
    checksum = fields["sha256"] if kind == "release" else fields["manifest_sha256"]
    if not re.fullmatch(r"[0-9a-f]{64}", checksum):
        raise ValueError("SOURCE checksum must be 64 lowercase hexadecimal digits")
    if kind == "release":
        if not fields["url"].startswith("https://"):
            raise ValueError("release source must use HTTPS")
        fields["root"] = "mandoc-" + fields["version"]
    else:
        for key in ("module", "root", "manifest"):
            component(fields[key])
        datetime.datetime.strptime(fields["date"], "%Y-%m-%d %H:%M:%S UTC")
        if fields["cvsroot"] != ":ext:anoncvs@mandoc.bsd.lv:/cvs":
            raise ValueError("CVS source must use the pinned official anonymous server")
    return fields


def extract_archive(archive, directory, root):
    """Extract regular files only, with a fixed root and no link/traversal paths."""
    with tarfile.open(archive, "r:*") as stream:
        members = stream.getmembers()
        seen = set()
        for member in members:
            path = PurePosixPath(member.name)
            if (path.is_absolute() or ".." in path.parts or "\\" in member.name
                    or not path.parts or path.parts[0] != root
                    or not (member.isfile() or member.isdir())
                    or path in seen):
                raise ValueError(f"unsafe or duplicate archive member: {member.name!r}")
            seen.add(path)
        # All names and types were checked before the first filesystem write.
        for member in members:
            target = directory.joinpath(*PurePosixPath(member.name).parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with stream.extractfile(member) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output)
                target.chmod(member.mode & 0o777)
    unpacked = directory / root
    if not unpacked.is_dir():
        raise ValueError("archive does not contain the locked source root")
    return unpacked


def source_files(root):
    """The vendored subset excludes upstream regression corpus and CVS metadata."""
    files = {}
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        if relative.parts[0] == "regress" or "CVS" in relative.parts:
            continue
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise ValueError(f"nonregular source path: {relative}")
        if path.is_file():
            files[relative.as_posix()] = path
    return files


def read_manifest(path, expected_checksum):
    if sha256(path) != expected_checksum:
        raise ValueError("CVS manifest checksum mismatch")
    entries = {}
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if not line or line.startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) != 3:
            raise ValueError(f"invalid CVS manifest line {number}")
        checksum, revision, name = fields
        relative = PurePosixPath(name)
        if (not re.fullmatch(r"[0-9a-f]{64}", checksum)
                or not re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", revision)
                or relative.is_absolute() or ".." in relative.parts
                or not relative.parts or "\\" in name or name != relative.as_posix()
                or name in entries):
            raise ValueError(f"invalid CVS manifest entry at line {number}")
        entries[name] = (checksum, revision)
    if not entries:
        raise ValueError("CVS manifest is empty")
    return entries


def verify_cvs_tree(root, entries):
    files = source_files(root)
    if files.keys() != entries.keys():
        raise ValueError(f"CVS file inventory mismatch: missing={sorted(entries.keys() - files.keys())}, "
                         f"extra={sorted(files.keys() - entries.keys())}")
    for name, path in files.items():
        checksum, revision = entries[name]
        if sha256(path) != checksum:
            raise ValueError(f"CVS content checksum mismatch: {name}")
        # Offline archives may omit CVS administration; the content manifest
        # remains authoritative. Live checkouts also prove each locked revision.
        metadata = path.parent / "CVS" / "Entries"
        if metadata.is_file():
            prefix = f"/{path.name}/{revision}/"
            if not any(line.startswith(prefix) for line in metadata.read_text().splitlines()):
                raise ValueError(f"CVS revision mismatch: {name}")


def acquire(root, source, directory, archive):
    entries = None
    if source["kind"] == "cvs":
        entries = read_manifest(root / "upstream" / source["manifest"], source["manifest_sha256"])
    if archive is not None:
        if source["kind"] == "release" and sha256(archive) != source["sha256"]:
            raise ValueError("release archive checksum mismatch")
        unpacked = extract_archive(archive, directory, source["root"])
    elif source["kind"] == "release":
        downloaded = directory / "upstream.tar.gz"
        subprocess.run(["curl", "--proto", "=https", "--tlsv1.2", "-fsSL", "--max-time", "120",
                        "-o", str(downloaded), source["url"]], check=True)
        if sha256(downloaded) != source["sha256"]:
            raise ValueError("release archive checksum mismatch")
        unpacked = extract_archive(downloaded, directory, source["root"])
    else:
        environment = dict(os.environ, CVS_RSH=str(root / "scripts" / "cvs-ssh"))
        subprocess.run([os.environ.get("CVS", "cvs"), "-Q", "-d", source["cvsroot"], "checkout",
                        "-D", source["date"], "-d", source["root"], source["module"]],
                       cwd=directory, env=environment, check=True)
        unpacked = directory / source["root"]
    if entries is not None:
        verify_cvs_tree(unpacked, entries)
    # Copy only the shipping source subset. This also removes CVS metadata before
    # patches run, and makes archive and live-checkout replay identical.
    staged = directory / "patched"
    staged.mkdir()
    for name, path in source_files(unpacked).items():
        destination = staged / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
    return staged


def apply_patches(root, staged):
    patches = root / "patches"
    for line in (patches / "series").read_text().splitlines():
        name = line.strip()
        if not name or name.startswith("#"):
            continue
        component(name)
        print(f"[sync-vendor] applying {name}", flush=True)
        with (patches / name).open("rb") as patch:
            subprocess.run(["patch", "--batch", "--fuzz=0", "-p1", "-d", str(staged)],
                           stdin=patch, check=True)


def compare_trees(left, right):
    expected, actual = source_files(left), source_files(right)
    differences = set(expected) ^ set(actual)
    differences.update(name for name in expected.keys() & actual.keys()
                       if expected[name].read_bytes() != actual[name].read_bytes())
    return sorted(differences)


def install_tree(staged, destination):
    """Keep the previous tree until the complete new tree has been staged."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_symlink():
        raise ValueError("vendor destination must not be a symlink")
    with tempfile.TemporaryDirectory(prefix=".sync-vendor-", dir=destination.parent) as owned:
        owned = Path(owned)
        replacement, backup = owned / "replacement", owned / "previous"
        shutil.copytree(staged, replacement)
        if destination.exists():
            destination.rename(backup)
        try:
            replacement.rename(destination)
        except OSError:
            if backup.exists():
                backup.rename(destination)
            raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", action="store_true", help="compare without modifying vendor")
    parser.add_argument("--archive", type=Path, help="use a local locked upstream archive without network")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    source = read_source(root / "upstream" / "SOURCE")
    destination = root / "vendor" / ("mandoc-" + source["version"])
    with tempfile.TemporaryDirectory(prefix="mant-sync-vendor-") as directory:
        staged = acquire(root, source, Path(directory), args.archive)
        apply_patches(root, staged)
        if args.verify:
            if not destination.is_dir() or destination.is_symlink():
                raise ValueError(f"missing or invalid vendor directory: {destination}")
            differences = compare_trees(staged, destination)
            if differences:
                raise ValueError("vendor differs from upstream + patches: " + ", ".join(differences))
            print("[sync-vendor] vendor is up-to-date")
        else:
            install_tree(staged, destination)
            print(f"[sync-vendor] replaced {destination}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError, tarfile.TarError) as error:
        print(f"sync-vendor: {error}", file=sys.stderr)
        sys.exit(1)
