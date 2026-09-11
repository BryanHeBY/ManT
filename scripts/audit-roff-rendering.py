#!/usr/bin/env python3
"""Source-bound, bounded content/geometry census against a pinned mandoc binary.

Does not build, download, modify old ledgers, or bless candidates. Unknown
framing, source execution and comparison limits remain observable coverage
gaps. Run on trusted local corpora: process limits are not a filesystem sandbox.
"""
from __future__ import annotations

import argparse
import bz2
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import gzip
import hashlib
import io
import json
import lzma
import math
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

from roff_content_compare import compare_content
from roff_layout_geometry import compare_layout_geometry
from roff_reference import MAX_INPUT_BYTES, reference_environment, run_renderer
from roff_rendering_frame import prepare_frame

ROOT = Path(__file__).resolve().parents[1]
EXTERNAL = re.compile(rb"^[.'][ \t]*(?:so|soquiet|mso)(?:[ \t]|$)", re.MULTILINE)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_hash(path):
    result = hashlib.sha256()
    with regular_stream(Path(path)) as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def regular_stream(path):
    if not stat.S_ISREG(path.stat().st_mode):
        raise ValueError("not a regular file")
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_NONBLOCK", 0))
    stream = os.fdopen(fd, "rb")
    if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
        stream.close()
        raise ValueError("file changed to a non-regular file")
    return stream


def decode_xz(encoded):
    """Bound decoder dictionary memory too; retain concatenated XZ streams."""
    output = bytearray()
    remaining = encoded
    while remaining:
        decoder = lzma.LZMADecompressor(memlimit=64 * 1024 * 1024)
        chunk = remaining
        while True:
            output.extend(decoder.decompress(chunk, max_length=MAX_INPUT_BYTES + 1 - len(output)))
            if len(output) > MAX_INPUT_BYTES:
                raise ValueError("decoded source exceeds 16 MiB")
            if decoder.eof:
                break
            if decoder.needs_input:
                raise EOFError("incomplete XZ stream")
            chunk = b""
        remaining = decoder.unused_data
        # XZ stream padding consists of complete groups of four zero bytes.
        padding = len(remaining) - len(remaining.lstrip(b"\0"))
        if padding % 4:
            raise ValueError("invalid XZ stream padding")
        remaining = remaining[padding:]
    return bytes(output)


def source_bytes(path):
    """Check/open/check and bounded decompression; never block on a FIFO."""
    with regular_stream(path) as stream:
        encoded = stream.read(MAX_INPUT_BYTES + 1)
    if len(encoded) > MAX_INPUT_BYTES:
        raise ValueError("compressed source exceeds 16 MiB")
    if encoded.startswith(b"\x28\xb5\x2f\xfd"):
        code, decoded, error = run_renderer(
            ["zstd", "-qdc"], 10, reference_environment(), encoded,
            binary_output=True, output_limit=MAX_INPUT_BYTES)
        if code:
            raise ValueError(f"bounded zstd decode failed: {error}")
    elif encoded.startswith(b"\xfd7zXZ\0"):
        decoded = decode_xz(encoded)
    else:
        opener = (gzip.open if encoded.startswith(b"\x1f\x8b") else
                  bz2.BZ2File if encoded.startswith(b"BZh") else None)
        if opener:
            with opener(io.BytesIO(encoded)) as stream:
                decoded = stream.read(MAX_INPUT_BYTES + 1)
        else:
            decoded = encoded
    if len(decoded) > MAX_INPUT_BYTES:
        raise ValueError("decoded source exceeds 16 MiB")
    return decoded, digest(encoded)


def census_inputs(args):
    rows = []
    args._manifest_sha256 = None
    if args.manifest:
        with regular_stream(args.manifest) as stream:
            snapshot = stream.read(128 * 1024 * 1024 + 1)
        if len(snapshot) > 128 * 1024 * 1024:
            raise ValueError("manifest exceeds 128 MiB")
        args._manifest_sha256 = digest(snapshot)
        identities = set()
        for line in snapshot.decode("utf-8").splitlines():
            if not line.strip():
                continue
            value = json.loads(line)
            identity, path = value["id"], value["source_path"]
            if not isinstance(identity, str) or not identity or identity in identities:
                raise ValueError("manifest IDs must be nonempty unique strings")
            if not isinstance(path, str) or not path:
                raise ValueError("manifest source_path must be a nonempty string")
            identities.add(identity)
            rows.append({"id": identity, "path": str(Path(path).resolve()),
                         "historicalSha256": value.get("source_sha256")})
    else:
        for path in sorted((ROOT / "tests/fixtures/roff/real").rglob("*")):
            if path.is_file() and re.search(r"\.[1-9][A-Za-z0-9]*(?:\.(?:gz|bz2|xz|zst))?$", path.name):
                rows.append({"id": str(path.relative_to(ROOT)), "path": str(path)})
    if args.max_pages:
        rows = rows[:args.max_pages]
    if not rows:
        raise ValueError("no source pages selected; empty coverage is not success")
    groups = {}
    for row in rows:
        groups.setdefault(row["path"], []).append(row)
    return list(groups.items())


def compact(value):
    return {key: item for key, item in value.items() if key != "findings"} | {
        "findings": value.get("findings", [])[:3],
        "findingsRetained": min(3, len(value.get("findings", []))),
        "findingsAvailable": len(value.get("findings", [])),
    }


def inspect(item, args):
    path, identities = item
    record = {"sourcePath": path, "identities": identities, "status": "uncovered"}
    artifacts = {}
    try:
        source, encoded_sha = source_bytes(Path(path))
        actual_sha = digest(source)
        source_text = source.decode("utf-8", errors="replace")
        record.update(sourceSha256=actual_sha, transportSha256=encoded_sha,
                      sourceValidUtf8=source == source_text.encode("utf-8"),
                      sourceBytes=len(source), historicalHashMatches=[
                          row.get("historicalSha256") == actual_sha for row in identities])
        if EXTERNAL.search(source):
            record["reason"] = "external-source-context: standalone .so/.soquiet/.mso is not comparable"
            return record, artifacts
        environment = reference_environment()
        commands = {
            "reference": [str(args.reference), "-Tutf8", "-O", f"width={args.width}"],
            "mant": [str(args.mant), "--input", "-", "--input-format", "roff",
                     "--format", "text", "--display", "direct", "--color", "never"],
        }
        outputs = {}
        for name, command in commands.items():
            code, output, error = run_renderer(command, args.timeout, environment,
                source, binary_output=True, output_limit=8 * 1024 * 1024)
            record[name] = {"code": code, "stdoutSha256": digest(output),
                            "stderrTextSha256": digest(error.encode()), "stderr": error[:512],
                            "stderrEncoding": "UTF-8 decoded with replacement by bounded runner"}
            outputs[name] = output.decode("utf-8", errors="replace")
            record[name]["validUtf8"] = output == outputs[name].encode()
            artifacts[name + ".stdout"] = output
            artifacts[name + ".stderr"] = error.encode()
        if record["mant"]["code"]:
            record.update(status="hard-failure", reason="mant-process-failure")
            return record, artifacts
        if record["reference"]["code"]:
            record.update(reason="reference-process-failure")
            return record, artifacts
        ref, ref_frame = prepare_frame(outputs["reference"], source_text, reference=True)
        mant, mant_frame = prepare_frame(outputs["mant"], source_text, reference=False)
        record["frames"] = {"reference": ref_frame, "mant": mant_frame}
        content = compare_content(ref, mant, source_text)
        geometry = compare_layout_geometry(ref, mant, source_text)
        # Check raw output as well: furniture masking must never conceal leaks.
        raw = compare_content(outputs["mant"], outputs["mant"], source_text)
        record["rawControlLeaks"] = raw.get("counts", {}).get("control-leak", 0)
        record["rawControlCoverage"] = {"status": raw["status"], "coverage": raw.get("coverage", {})}
        record["content"] = compact(content)
        record["geometry"] = compact(geometry)
        statuses = [content["status"], geometry["status"], ref_frame["status"], mant_frame["status"], raw["status"]]
        if raw["status"] == "hard-failure" or content["status"] == "hard-failure":
            record["status"] = "hard-failure"
        elif ("review" in statuses or not record["sourceValidUtf8"] or
              any(not record[name]["validUtf8"] for name in commands)):
            record["status"] = "review"
        elif all(status == "covered" for status in statuses):
            record["status"] = "clean"
        else:
            record["status"] = "partial"
        artifacts["source.roff"] = source
        artifacts["comparison.json"] = (json.dumps({"content": content, "geometry": geometry},
                                       ensure_ascii=False, indent=2) + "\n").encode()
    except (OSError, ValueError, EOFError, lzma.LZMAError) as error:
        record.update(reason="source-or-comparison-error", error=str(error)[:512])
    return record, artifacts


def main():
    if sys.argv[1:] == ["--self-test"]:
        import unittest
        suite = unittest.defaultTestLoader.discover(str(ROOT / "scripts"), pattern="test_audit_roff_rendering.py")
        return int(not unittest.TextTestRunner().run(suite).wasSuccessful())
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, help="Otherwise scan checked-in real fixtures")
    parser.add_argument("--mant", type=Path, default=ROOT / "target/release/mant")
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--reference-id", required=True, help="Fixed upstream revision/date, not moving HEAD")
    parser.add_argument("--output", type=Path, required=True, help="New evidence directory")
    parser.add_argument("--width", type=int, default=200)
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--max-pages", type=int)
    parser.add_argument("--artifact-pages", type=int, default=32)
    parser.add_argument("--verify", action="store_true", help="Also fail on candidates or incomplete coverage")
    args = parser.parse_args()
    if not 1 <= args.workers <= 4 or not 20 <= args.width <= 1000 or not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("workers 1..4, width 20..1000, and positive timeout required")
    if args.artifact_pages < 0 or (args.max_pages is not None and args.max_pages < 1):
        parser.error("invalid page budget")
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"schema": "mant.roff-rendering-census/v1", "status": "audit-error", "coverageComplete": False}
    try:
        return census(args, report)
    except Exception as error:
        # A broken input/formatter/comparator must leave a machine-readable
        # terminal record, never a missing summary that can resemble completion.
        report.update(status="audit-error", coverageComplete=False,
                      finished=datetime.now(timezone.utc).isoformat(),
                      auditError={"type": type(error).__name__, "message": str(error)[:1024]})
        results = args.output / "results.jsonl"
        if results.is_file():
            report["resultsSha256"] = file_hash(results)
        (args.output / "summary.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"summary": str(args.output / "summary.json"), "error": report["auditError"]}))
        return 1


def census(args, report):
    args.mant, args.reference = args.mant.resolve(strict=True), args.reference.resolve(strict=True)
    inputs = census_inputs(args)
    binaries = {name: {"path": str(path), "sha256": file_hash(path)}
                for name, path in (("mant", args.mant), ("reference", args.reference))}
    producer = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).splitlines()
    report.update({"schema": "mant.roff-rendering-census/v1", "producerCommit": producer,
              "producerDirtyPaths": dirty, "started": datetime.now(timezone.utc).isoformat(),
              "binaries": binaries, "referenceIdentity": args.reference_id,
              "manifestSha256": args._manifest_sha256,
              "comparators": {name: file_hash(ROOT / "scripts" / name) for name in (
                  "roff_content_compare.py", "roff_layout_geometry.py", "roff_rendering_frame.py",
                  "roff_reference.py", "audit-roff-rendering.py")},
              "parameters": {key: str(value) if isinstance(value, Path) else value
                             for key, value in vars(args).items() if not key.startswith("_")},
              "physicalPages": len(inputs), "logicalPages": sum(len(rows) for _, rows in inputs),
              "environment": reference_environment()})
    counts, dimensions = Counter(), Counter()
    saved_bytes = saved_pages = 0
    with (args.output / "results.jsonl").open("w", encoding="utf-8") as stream, ThreadPoolExecutor(args.workers) as pool:
        # Executor.map preserves source order, but eagerly submits all futures.
        # Batches bound queued rendered output independently of corpus size.
        for start in range(0, len(inputs), args.workers * 4):
            for record, artifacts in pool.map(lambda item: inspect(item, args), inputs[start:start + args.workers * 4]):
                counts[record["status"]] += len(record["identities"])
                for dimension in ("content", "geometry"):
                    value = record.get(dimension, {})
                    dimensions[dimension + ":" + value.get("status", "uncovered")] += len(record["identities"])
                size = sum(len(value) for value in artifacts.values())
                if record["status"] != "clean" and artifacts and saved_pages < args.artifact_pages and saved_bytes + size <= 64 * 1024 * 1024:
                    directory = args.output / f"candidate-{saved_pages:03d}"
                    directory.mkdir()
                    for name, data in artifacts.items():
                        (directory / name).write_bytes(data)
                    record["artifacts"] = str(directory)
                    saved_pages += 1
                    saved_bytes += size
                stream.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
                report["statusCounts"] = dict(counts)
            stream.flush()
            if start % (args.workers * 4 * 16) == 0:
                print(f"{min(start + args.workers * 4, len(inputs))}/{len(inputs)} physical pages: {dict(counts)}", flush=True)
    report.update(status="completed", coverageComplete=all(status == "clean" for status in counts),
                  finished=datetime.now(timezone.utc).isoformat(), statusCounts=dict(counts),
                  dimensions=dict(dimensions), artifactBytes=saved_bytes, artifactPages=saved_pages,
                  resultsSha256=file_hash(args.output / "results.jsonl"),
                  binariesUnchanged=all(file_hash(Path(v["path"])) == v["sha256"] for v in binaries.values()),
                  comparatorsUnchanged=all(file_hash(ROOT / "scripts" / name) == sha for name, sha in report["comparators"].items()),
                  manifestUnchanged=args.manifest is None or file_hash(args.manifest) == args._manifest_sha256)
    identity_stable = report["binariesUnchanged"] and report["comparatorsUnchanged"] and report["manifestUnchanged"]
    report["coverageComplete"] = report["coverageComplete"] and identity_stable
    (args.output / "summary.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"summary": str(args.output / "summary.json"), "counts": dict(counts)}))
    return int(not identity_stable or counts["hard-failure"] > 0 or
               (args.verify and any(value for status, value in counts.items() if status != "clean")))


if __name__ == "__main__":
    sys.exit(main())
