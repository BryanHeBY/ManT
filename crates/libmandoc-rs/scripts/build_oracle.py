#!/usr/bin/env python3
"""Build a pristine mandoc oracle from a locked archive under target/."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

import sync_vendor


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts"))
import mandoc_oracle  # noqa: E402


RECIPE = "crates/libmandoc-rs/upstream/oracle/recipe.json"


def exact_keys(value: dict, expected: set[str], label: str) -> None:
    if not isinstance(value, dict) or set(value) != expected:
        raise ValueError(f"{label} has missing or unknown fields")


def read_recipe(root: Path) -> dict:
    value = json.loads((root / RECIPE).read_text())
    exact_keys(value, {
        "schema", "archiveRoot", "configureCommand", "buildCommand",
        "environment", "evidenceFiles", "profiles",
    }, "oracle recipe")
    if value["schema"] != "libmandoc-rs.oracle-build-recipe/v1" or value["archiveRoot"] != "mandoc":
        raise ValueError("unsupported oracle recipe")
    if (not isinstance(value["environment"], dict)
            or set(value["environment"]) != {"CC", "LANG", "LC_ALL", "TZ"}):
        raise ValueError("oracle recipe environment must be a closed allowlist")
    for field in ("configureCommand", "buildCommand", "evidenceFiles", "profiles"):
        items = value[field]
        if not isinstance(items, list) or not items or not all(isinstance(x, str) and x for x in items):
            raise ValueError(f"oracle recipe {field} is invalid")
    if len(set(value["profiles"])) != len(value["profiles"]):
        raise ValueError("oracle recipe profiles contain duplicates")
    return value


def checked_target(root: Path, value: Path, label: str) -> Path:
    path = value.resolve()
    target = (root / "target").resolve()
    if not path.is_relative_to(target) or path == target:
        raise ValueError(f"{label} must be a dedicated path below {target}")
    return path


def command_identity(command: str, environment: dict[str, str]) -> tuple[str, str]:
    path = shutil.which(command, path=environment["PATH"])
    if path is None:
        raise ValueError(f"required build tool is unavailable: {command}")
    result = subprocess.run(
        [path, "--version"], env=environment, check=True, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    return str(Path(path).resolve()), result.stdout.strip()


def build(root: Path, archive: Path, output: Path, identity: str, jobs: int) -> dict:
    root = root.resolve()
    archive = checked_target(root, archive, "source archive")
    output = checked_target(root, output, "oracle output")
    if output.exists():
        raise ValueError("oracle output directory must not already exist")
    if jobs < 1 or jobs > 64:
        raise ValueError("build jobs must be in the range 1..64")
    recipe = read_recipe(root)
    source = sync_vendor.read_source(root / "crates/libmandoc-rs/upstream/SOURCE")
    if source["kind"] != "cvs":
        raise ValueError("oracle recipe requires a CVS source lock")
    if "archive_sha256" in source and sync_vendor.sha256(archive) != source["archive_sha256"]:
        raise ValueError("source archive does not match SOURCE")
    entries = sync_vendor.read_manifest(
        root / "crates/libmandoc-rs/upstream" / source["manifest"],
        source["manifest_sha256"],
    )
    environment = {"PATH": os.environ.get("PATH", "")}
    environment.update(recipe["environment"])
    compiler_path, compiler_version = command_identity(recipe["environment"]["CC"], environment)
    toolchain = {}
    for name in ("make", "ld"):
        toolchain[name] = command_identity(name, environment)

    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".oracle-build-", dir=output.parent) as temporary:
        candidate = Path(temporary) / "candidate"
        candidate.mkdir()
        extracted = sync_vendor.extract_archive(archive, candidate, recipe["archiveRoot"])
        sync_vendor.verify_cvs_tree(extracted, entries)
        source_tree = candidate / "source"
        extracted.rename(source_tree)
        configure_command = recipe["configureCommand"]
        # The archive is compiled in a disposable directory. Map that path in
        # DWARF and __FILE__ so the registered artifact can survive cargo clean
        # and be reproduced from the same pristine archive and toolchain.
        build_command = [
            item.format(jobs=jobs, source=str(source_tree))
            for item in recipe["buildCommand"]
        ]
        recorded_build_command = [
            item.format(jobs=jobs, source="{source}")
            for item in recipe["buildCommand"]
        ]
        subprocess.run(configure_command, cwd=source_tree, env=environment, check=True)
        subprocess.run(build_command, cwd=source_tree, env=environment, check=True)
        binary = source_tree / "mandoc"
        if not binary.is_file() or binary.is_symlink():
            raise ValueError("upstream build did not produce a regular mandoc binary")
        shutil.copy2(binary, candidate / "mandoc")
        build_record = {
            "schema": "libmandoc-rs.oracle-build/v1",
            "identity": identity,
            "sourceArchive": str(archive.relative_to(root)),
            "sourceArchiveSha256": sync_vendor.sha256(archive),
            "compiler": {"path": compiler_path, "version": compiler_version},
            "tools": {name: {"path": item[0], "version": item[1]} for name, item in toolchain.items()},
            "environment": recipe["environment"],
            "configureCommand": configure_command,
            "buildCommand": recorded_build_command,
            "productPatchesApplied": False,
        }
        (candidate / "BUILD.json").write_text(json.dumps(build_record, sort_keys=True, indent=2) + "\n")
        candidate.rename(output)

    evidence_names = {
        "config.h": "configHeader",
        "config.log": "configLog",
        "Makefile.local": "makefileLocal",
    }
    evidence = {
        evidence_names.get(name, Path(name).name.replace(".", "")):
        str((output / "source" / name).relative_to(root))
        for name in recipe["evidenceFiles"]
    }
    evidence["buildRecord"] = str((output / "BUILD.json").relative_to(root))
    attestation = mandoc_oracle.create_attestation(
        root,
        identity,
        output / "mandoc",
        str(archive.relative_to(root)),
        RECIPE,
        evidence,
        recipe["configureCommand"],
        [item.format(jobs=jobs, source="{source}") for item in recipe["buildCommand"]],
        compiler_path,
        compiler_version,
        recipe["profiles"],
        "crates/libmandoc-rs/upstream/CVS_INVENTORY.json"
        if (root / "crates/libmandoc-rs/upstream/CVS_INVENTORY.json").is_file() else None,
    )
    (output / "attestation.json").write_text(json.dumps(attestation, sort_keys=True, indent=2) + "\n")
    return attestation


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=ROOT, help=argparse.SUPPRESS)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--identity", required=True)
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    try:
        value = build(args.repository, args.archive, args.output, args.identity, args.jobs)
    except (OSError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"build-oracle: {error}", file=sys.stderr)
        return 1
    print(json.dumps(value, sort_keys=True, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
