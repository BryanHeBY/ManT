#!/usr/bin/env python3
"""Create and verify trusted identities for pristine mandoc CVS oracles."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
from pathlib import Path
import re
import sys


SCHEMA = "mant.mandoc-oracle-attestation/v1"
PROFILES = {"ascii", "utf8", "html"}
REGISTRY = "crates/libmandoc-rs/upstream/oracle/registry.json"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def repository_path(root: Path, value: str, label: str) -> Path:
    if not value or Path(value).is_absolute() or ".." in Path(value).parts:
        raise ValueError(f"{label} must be a repository-relative path")
    path = (root / value).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError(f"{label} escapes the repository")
    return path


def exact_keys(value: dict, expected: set[str], label: str) -> None:
    if not isinstance(value, dict) or set(value) != expected:
        raise ValueError(f"{label} has missing or unknown fields")


def hash_record(root: Path, relative: str, label: str) -> dict[str, str]:
    path = repository_path(root, relative, label)
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"{label} is missing or not a regular file")
    return {"path": relative, "sha256": sha256(path)}


def validate_hash_record(root: Path, record: dict, label: str) -> Path:
    exact_keys(record, {"path", "sha256"}, label)
    if not re.fullmatch(r"[0-9a-f]{64}", record["sha256"]):
        raise ValueError(f"{label} has an invalid SHA-256")
    path = repository_path(root, record["path"], label)
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"{label} is missing or not a regular file")
    if sha256(path) != record["sha256"]:
        raise ValueError(f"{label} hash mismatch")
    return path


def create_attestation(
    root: Path,
    identity: str,
    binary: Path,
    archive_relative: str,
    recipe_relative: str,
    build_evidence: dict[str, str],
    configure_command: list[str],
    build_command: list[str],
    compiler_command: str,
    compiler_version: str,
    profiles: list[str],
    inventory_relative: str | None = None,
) -> dict:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.+-]*", identity):
        raise ValueError("oracle identity contains unsupported characters")
    if not binary.is_file() or binary.is_symlink():
        raise ValueError("oracle binary is missing or not a regular file")
    if not profiles or set(profiles) - PROFILES or len(set(profiles)) != len(profiles):
        raise ValueError("oracle profiles must be unique supported names")
    for command in (configure_command, build_command):
        if not command or any(not isinstance(item, str) or not item for item in command):
            raise ValueError("oracle build commands must be non-empty string arrays")
    evidence = {}
    for name, relative in sorted(build_evidence.items()):
        if not re.fullmatch(r"[A-Za-z][A-Za-z0-9]*", name):
            raise ValueError("invalid build evidence name")
        evidence[name] = hash_record(root, relative, f"build evidence {name}")
    source = {
        "lock": hash_record(root, "crates/libmandoc-rs/upstream/SOURCE", "SOURCE lock"),
        "manifest": hash_record(root, "crates/libmandoc-rs/upstream/FILES", "FILES manifest"),
        "archive": hash_record(root, archive_relative, "source archive"),
        "inventory": hash_record(root, inventory_relative, "CVS inventory") if inventory_relative else None,
        "pristine": True,
    }
    return {
        "schema": SCHEMA,
        "identity": identity,
        "source": source,
        "recipe": {
            "file": hash_record(root, recipe_relative, "oracle build recipe"),
            "configureCommand": configure_command,
            "buildCommand": build_command,
        },
        "toolchain": {
            "compilerCommand": compiler_command,
            "compilerVersion": compiler_version,
        },
        "platform": {
            "system": platform.system(),
            "machine": platform.machine(),
        },
        "buildEvidence": evidence,
        "artifact": {"sha256": sha256(binary)},
        "profiles": sorted(profiles),
    }


def preflight(
    root: Path,
    binary: Path,
    archive: Path,
    attestation: Path,
    expected_identity: str | None = None,
    profile: str = "utf8",
) -> dict:
    root = root.resolve()
    if profile not in PROFILES:
        raise ValueError(f"unsupported oracle profile: {profile}")
    if not attestation.is_file() or attestation.is_symlink():
        raise ValueError("oracle attestation is missing or not a regular file")
    try:
        value = json.loads(attestation.read_text())
    except json.JSONDecodeError as error:
        raise ValueError(f"invalid oracle attestation JSON: {error}") from error
    exact_keys(value, {
        "schema", "identity", "source", "recipe", "toolchain", "platform",
        "buildEvidence", "artifact", "profiles",
    }, "oracle attestation")
    if value["schema"] != SCHEMA:
        raise ValueError("unsupported oracle attestation schema")
    if expected_identity is not None and value["identity"] != expected_identity:
        raise ValueError("oracle identity does not match the requested reference identity")
    registry_path = root / REGISTRY
    try:
        registry = json.loads(registry_path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"trusted oracle registry is unavailable: {error}") from error
    exact_keys(registry, {"schema", "attestations"}, "oracle registry")
    if registry["schema"] != "mant.mandoc-oracle-registry/v1" or not isinstance(registry["attestations"], dict):
        raise ValueError("unsupported oracle registry")
    registration = registry["attestations"].get(value["identity"])
    if registration is None:
        raise ValueError("oracle attestation is not registered")
    exact_keys(registration, {"path", "sha256", "status"}, "oracle registration")
    registered_path = repository_path(root, registration["path"], "registered attestation")
    if (registration["status"] != "active"
            or registered_path != attestation.resolve()
            or registration["sha256"] != sha256(attestation)):
        raise ValueError("oracle attestation is not an active trusted registration")
    if profile not in value["profiles"] or set(value["profiles"]) - PROFILES:
        raise ValueError("oracle attestation does not authorize the requested profile")

    exact_keys(value["source"], {"lock", "manifest", "archive", "inventory", "pristine"}, "oracle source")
    if value["source"]["pristine"] is not True:
        raise ValueError("oracle source is not attested as pristine")
    validate_hash_record(root, value["source"]["lock"], "SOURCE lock")
    validate_hash_record(root, value["source"]["manifest"], "FILES manifest")
    if value["source"]["inventory"] is not None:
        validate_hash_record(root, value["source"]["inventory"], "CVS inventory")
    archive_record = value["source"]["archive"]
    exact_keys(archive_record, {"path", "sha256"}, "source archive")
    if not archive.is_file() or archive.is_symlink() or sha256(archive) != archive_record["sha256"]:
        raise ValueError("source archive does not match the oracle attestation")

    exact_keys(value["recipe"], {"file", "configureCommand", "buildCommand"}, "oracle recipe")
    validate_hash_record(root, value["recipe"]["file"], "oracle build recipe")
    for key in ("configureCommand", "buildCommand"):
        command = value["recipe"][key]
        if not isinstance(command, list) or not command or not all(isinstance(x, str) and x for x in command):
            raise ValueError(f"oracle recipe {key} is invalid")
    exact_keys(value["toolchain"], {"compilerCommand", "compilerVersion"}, "oracle toolchain")
    if not all(isinstance(value["toolchain"][key], str) and value["toolchain"][key]
               for key in value["toolchain"]):
        raise ValueError("oracle toolchain identity is incomplete")
    exact_keys(value["platform"], {"system", "machine"}, "oracle platform")
    if value["platform"] != {"system": platform.system(), "machine": platform.machine()}:
        raise ValueError("oracle platform does not match this host")
    if not isinstance(value["buildEvidence"], dict) or not value["buildEvidence"]:
        raise ValueError("oracle build evidence is empty")
    for name, record in value["buildEvidence"].items():
        validate_hash_record(root, record, f"build evidence {name}")
    exact_keys(value["artifact"], {"sha256"}, "oracle artifact")
    if (not re.fullmatch(r"[0-9a-f]{64}", value["artifact"]["sha256"])
            or not binary.is_file() or binary.is_symlink()
            or sha256(binary) != value["artifact"]["sha256"]):
        raise ValueError("oracle binary does not match the attestation")
    return {
        "schema": "mant.mandoc-oracle-preflight/v1",
        "identity": value["identity"],
        "profile": profile,
        "attestation": {"path": str(attestation), "sha256": sha256(attestation)},
        "registry": {"path": str(registry_path), "sha256": sha256(registry_path)},
        "binary": {"path": str(binary), "sha256": sha256(binary)},
        "archive": {"path": str(archive), "sha256": sha256(archive)},
        "sourceLockSha256": value["source"]["lock"]["sha256"],
        "manifestSha256": value["source"]["manifest"]["sha256"],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[1], help=argparse.SUPPRESS)
    parser.add_argument("--attestation", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--identity")
    parser.add_argument("--profile", choices=sorted(PROFILES), default="utf8")
    args = parser.parse_args()
    try:
        evidence = preflight(
            args.repository, args.binary.resolve(), args.archive.resolve(),
            args.attestation.resolve(), args.identity, args.profile,
        )
    except (OSError, ValueError) as error:
        print(f"mandoc-oracle: {error}", file=sys.stderr)
        return 1
    print(json.dumps(evidence, sort_keys=True, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
