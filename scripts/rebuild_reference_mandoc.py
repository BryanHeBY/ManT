#!/usr/bin/env python3
"""Restore the active pristine mandoc oracle using its locked build recipe."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile

import mandoc_oracle


ROOT = Path(__file__).resolve().parents[1]
REFERENCE = "target/mandoc-migration/reference/mandoc"


def active_attestation(root: Path, identity: str | None = None) -> tuple[Path, dict]:
    registry = json.loads((root / mandoc_oracle.REGISTRY).read_text())
    mandoc_oracle.exact_keys(registry, {"schema", "attestations"}, "oracle registry")
    if registry["schema"] != "mant.mandoc-oracle-registry/v1":
        raise ValueError("unsupported oracle registry")
    candidates = []
    for name, registration in registry["attestations"].items():
        mandoc_oracle.exact_keys(registration, {"path", "sha256", "status"}, "oracle registration")
        if registration["status"] != "active" or (identity is not None and name != identity):
            continue
        path = mandoc_oracle.validate_hash_record(
            root, {key: registration[key] for key in ("path", "sha256")}, "registered attestation",
        )
        value = json.loads(path.read_text())
        if value["identity"] != name or value["source"]["pristine"] is not True:
            raise ValueError("registered oracle is not a pristine identity")
        if value["platform"] != {"system": platform.system(), "machine": platform.machine()}:
            continue
        for key, label in (("lock", "SOURCE lock"), ("manifest", "FILES manifest"),
                           ("inventory", "CVS inventory")):
            record = value["source"][key]
            if record is not None:
                mandoc_oracle.validate_hash_record(root, record, label)
        mandoc_oracle.validate_hash_record(root, value["recipe"]["file"], "oracle build recipe")
        if set(value["profiles"]) != mandoc_oracle.PROFILES:
            raise ValueError("registered oracle must authorize ascii, utf8 and html")
        candidates.append((path, value))
    if len(candidates) != 1:
        raise ValueError("exactly one active registered oracle is required for this source and platform")
    return candidates[0]


def verified_copy(source: Path, destination: Path, expected_hash: str, label: str) -> None:
    if not source.is_file() or source.is_symlink() or mandoc_oracle.sha256(source) != expected_hash:
        raise ValueError(f"{label} does not match its registered SHA-256")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(prefix=".oracle-copy-", dir=destination.parent, delete=False) as output:
        temporary = Path(output.name)
    try:
        shutil.copy2(source, temporary)
        if mandoc_oracle.sha256(temporary) != expected_hash:
            raise ValueError(f"{label} changed during restoration")
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)


def locked_target(root: Path, relative: str, label: str) -> Path:
    path = mandoc_oracle.repository_path(root, relative, label)
    if not path.is_relative_to(root / "target"):
        raise ValueError(f"{label} must be below target")
    return path


def restore_archive(root: Path, value: dict, supplied: Path | None, cvs: str) -> Path:
    record = value["source"]["archive"]
    archive = locked_target(root, record["path"], "source archive")
    if supplied is not None:
        verified_copy(supplied, archive, record["sha256"], "source archive")
    if archive.is_file():
        if archive.is_symlink() or mandoc_oracle.sha256(archive) != record["sha256"]:
            raise ValueError("existing source archive does not match its registered SHA-256")
        return archive
    client = shutil.which(cvs)
    if client is None:
        raise ValueError("locked archive is missing; provide --archive or a CVS client with --cvs")
    source = {}
    for line in (root / "crates/libmandoc-rs/upstream/SOURCE").read_text().splitlines():
        if line and not line.startswith("#"):
            key, entry = line.split("=", 1)
            source[key.strip()] = entry.strip()
    if source.get("kind") != "cvs":
        raise ValueError("reference restoration requires the locked CVS source")
    archive.parent.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".oracle-freeze-", dir=archive.parent.parent) as work:
        scratch = Path(work)
        subprocess.run([
            str(root / "crates/libmandoc-rs/scripts/freeze-cvs-snapshot"),
            "--repository", str(root), "--cvs", client, "--cutoff", source["date"],
            "--work", str(scratch / "checkouts"), "--output", str(scratch / "frozen"),
        ], check=True)
        for name, key in (("SOURCE", "lock"), ("FILES", "manifest"), ("CVS_INVENTORY.json", "inventory")):
            if mandoc_oracle.sha256(scratch / "frozen" / name) != value["source"][key]["sha256"]:
                raise ValueError(f"fresh CVS snapshot does not reproduce locked {name}")
        verified_copy(scratch / "frozen" / source["archive"], archive, record["sha256"], "source archive")
    return archive


def verify_all(root: Path, binary: Path, archive: Path, attestation: Path, identity: str) -> None:
    for profile in sorted(mandoc_oracle.PROFILES):
        mandoc_oracle.preflight(root, binary, archive, attestation, identity, profile)


def restore(root: Path, supplied_archive: Path | None = None, cvs: str = "cvs", identity: str | None = None) -> dict:
    root = root.resolve()
    attestation, value = active_attestation(root, identity)
    archive = restore_archive(root, value, supplied_archive, cvs)
    reference = root / REFERENCE
    try:
        verify_all(root, reference, archive, attestation, value["identity"])
    except ValueError:
        build_record = locked_target(root, value["buildEvidence"]["buildRecord"]["path"], "build record")
        output = build_record.parent
        candidate = output / "mandoc"
        try:
            verify_all(root, candidate, archive, attestation, value["identity"])
        except ValueError:
            if output.exists():
                # Keep failed evidence for inspection instead of overwriting it.
                backup = Path(tempfile.mkdtemp(prefix=".oracle-old-build-", dir=output.parent))
                output.rename(backup / "build")
            parallelism = [
                match[1] for argument in value["recipe"]["buildCommand"]
                if (match := re.fullmatch(r"-j([0-9]+)", argument)) is not None
            ]
            if len(parallelism) != 1:
                raise ValueError("registered build recipe must contain exactly one -j job count")
            subprocess.run([
                str(root / "crates/libmandoc-rs/scripts/build-oracle"),
                "--repository", str(root), "--archive", str(archive),
                "--output", str(output), "--identity", value["identity"], "--jobs", parallelism[0],
            ], check=True)
            if json.loads((output / "attestation.json").read_text()) != value:
                raise ValueError("rebuilt candidate differs from registered identity; automatic registration is forbidden")
            verify_all(root, candidate, archive, attestation, value["identity"])
        verified_copy(candidate, reference, value["artifact"]["sha256"], "oracle binary")
    verify_all(root, reference, archive, attestation, value["identity"])
    return {"identity": value["identity"], "binary": str(reference), "sha256": value["artifact"]["sha256"],
            "profiles": sorted(mandoc_oracle.PROFILES)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="previously saved exact pristine archive")
    parser.add_argument("--cvs", default=os.environ.get("CVS", "cvs"))
    parser.add_argument("--identity", help="require this active registered identity")
    args = parser.parse_args()
    try:
        value = restore(ROOT, args.archive, args.cvs, args.identity)
    except (OSError, ValueError, KeyError, json.JSONDecodeError, subprocess.CalledProcessError) as error:
        print(f"rebuild-reference-mandoc: {error}", file=sys.stderr)
        return 1
    print(json.dumps(value, sort_keys=True, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
