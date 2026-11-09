"""Record the five real public states for any completed native checkpoint."""
from base64 import b64decode
from collections import Counter
from itertools import islice
import json
from pathlib import Path
import subprocess
from tempfile import TemporaryFile
from time import monotonic, sleep

from scripts.roff.fixtures.reference_recipes import digest
from .compare import (STRICT, UnsupportedExtraction, group_witness, native_prose, product_prose)
from .manifest import jsonl, row_bytes
from .oracle import BATCH, atomic_output, check_checkpoint, stream_bytes

ROUTES = ("roff", "default-1", "default-2", "addressable-1", "addressable-2")


def verdict(source, oracle, observations, binding):
    recipes = binding["recipes"]
    streams = oracle["receipts"]
    utf8 = next(raw for key, raw in streams.items()
                if recipes[key]["profile"] == "utf8" and recipes[key]["width"] == 160)
    tree = next(b64decode(raw["stdout"]["base64"], validate=True) for key, raw in streams.items()
                if recipes[key]["profile"] == "tree")
    status = {key: raw["code"] for key, raw in streams.items()}
    result = dict(nativeStatuses=status, nativeGroups=group_witness(tree),
                  routes={})
    comparable = not STRICT.isdisjoint(source["cohorts"])
    expected = None
    if comparable:
        try:
            expected = native_prose(utf8, tree)
        except UnsupportedExtraction as error:
            result["comparisonReview"] = str(error)
    else:
        result["comparisonReview"] = "structured layout or non-prose recipe"
    result["nativeProse"] = expected
    if "error" in observations:
        result["productError"] = observations["error"]
    for route in ROUTES:
        row = observations.get(route)
        if row is None:
            state = "blocked-by-upstream-failure"
        elif not all(row[key] for key in ("jsonEquivalent", "validDocument", "ansiEquivalent")):
            state = "public-invariant-failure"
        elif expected is None:
            state = "review-extractor"
        else:
            try:
                state = ("exact" if product_prose(row["plain"]) == expected["rows"]
                         else "strict-row-difference")
            except UnsupportedExtraction:
                state = "strict-row-difference"
        result["routes"][route] = state
    return result


def product_batch(binary, chunk, checkpoint, output, binding):
    request = b"".join(row_bytes(dict(id=row["sourceSha256"], source=row["source"]))
                       for row in chunk)
    with TemporaryFile() as stdout:
        try:
            captured = subprocess.run([str(binary)], input=request, stdout=stdout,
                                      stderr=subprocess.PIPE, timeout=60)
            code, stderr = captured.returncode, captured.stderr
        except subprocess.TimeoutExpired as error:
            code, stderr = None, error.stderr or b""
        stdout.seek(0)
        if code != 0:
            (output / f"{checkpoint.stem}.error.json").write_bytes(row_bytes(
                dict(code=code, stdout=stream_bytes(stdout.read()), stderr=stream_bytes(stderr))))
            raise RuntimeError(f"profile failed with {code}")
        yield from observe_checkpoint(chunk, checkpoint, stdout, binding)


def observe_checkpoint(chunk, checkpoint, records, binding):
    catalog = list(binding["recipes"].items())
    for source, native in zip(chunk, jsonl(checkpoint), strict=True):
        source_hash = source["sourceSha256"]
        check_checkpoint(source, native, catalog)
        observations = {}
        while True:
            row = json.loads(next(records))
            if row["id"] != source_hash:
                raise ValueError("product response source order changed")
            if "end" in row:
                break
            observations[row.get("route", "error")] = row
        facts = verdict(source, native, observations, binding)
        yield dict(sourceSha256=source_hash, observations=observations, verdict=facts)
    if next(records, None) is not None:
        raise ValueError("product profile returned unsolicited observations")


def collect_product(directory, binary, name, *, wall_budget, artifact_budget, follow=False):
    native = directory / "oracle" / "full"
    binding = json.loads((native / "binding.json").read_bytes())
    output = directory / "product" / name
    output.mkdir(parents=True, exist_ok=True)
    binary = binary.resolve()
    product = dict(binary=str(binary), sha256=digest(binary.read_bytes()), native=binding,
                   profiles=list(ROUTES))
    path = output / "binding.json"
    if path.exists() and json.loads(path.read_bytes()) != product:
        raise ValueError("product binary or oracle cache changed")
    path.write_bytes(row_bytes(product))
    (output / "budget.json").write_bytes(row_bytes(dict(
        timeoutSeconds=60, wallSeconds=wall_budget, artifactBytes=artifact_budget,
        cacheDirectory=str(output.resolve()))))
    start = monotonic()
    size = sum(p.stat().st_size for p in output.glob("*.jsonl.gz"))
    completed = 0
    routes = Counter()
    stream = jsonl(directory / "sources.jsonl.gz")
    for index, chunk in enumerate(iter(lambda: list(islice(stream, BATCH)), [])):
        checkpoint = native / f"{index:06}.jsonl.gz"
        while (follow and not checkpoint.exists() and not (native / "summary.json").exists()
               and monotonic() - start < wall_budget):
            sleep(.25)
        if not checkpoint.exists() or monotonic() - start >= wall_budget:
            break
        result = output / checkpoint.name
        if not result.exists():
            if size >= artifact_budget:
                break
            with atomic_output(result) as writer:
                for row in product_batch(binary, chunk, checkpoint, output, binding):
                    writer.write(row_bytes(row))
            size += result.stat().st_size
        for source, row in zip(chunk, jsonl(result), strict=True):
            if source["sourceSha256"] != row["sourceSha256"]:
                raise ValueError("cached product checkpoint source order changed")
            completed += 1
            routes.update(row["verdict"]["routes"].values())
        (output / "progress.json").write_bytes(row_bytes(dict(
            completed=completed, routes=dict(routes), artifactBytes=size,
            wallSeconds=monotonic() - start)))
    planned = json.loads((directory / "manifest.json").read_bytes())["sources"]
    summary = dict(planned=planned, completed=completed, uncovered=planned - completed,
                   routes=dict(routes), artifactBytes=size, wallSeconds=monotonic() - start)
    (output / "summary.json").write_bytes(row_bytes(summary))
    return summary


def compare_products(directory):
    """Compare each public state to the same oracle, retaining every difference."""
    baseline = directory / "product" / "baseline"
    candidate = directory / "product" / "candidate"
    before = json.loads((baseline / "binding.json").read_bytes())
    after = json.loads((candidate / "binding.json").read_bytes())
    if before["native"] != after["native"]:
        raise ValueError("product observations use different native evidence")
    totals = Counter()
    states = {route: Counter() for route in ROUTES}
    pairs = 0
    with atomic_output(directory / "classification-differences.jsonl.gz") as differences:
        for chunk in sorted(candidate.glob("*.jsonl.gz")):
            previous = baseline / chunk.name
            if not previous.exists():
                break
            for left, right in zip(jsonl(previous), jsonl(chunk), strict=True):
                if left["sourceSha256"] != right["sourceSha256"]:
                    raise ValueError("baseline/candidate source order changed")
                expected = left["verdict"]["nativeProse"]
                if expected != right["verdict"]["nativeProse"]:
                    raise ValueError("baseline/candidate native projection changed")
                pairs += 1
                changes = {}
                for route in ROUTES:
                    old = public_state(left["observations"].get(route), expected)
                    new = public_state(right["observations"].get(route), expected)
                    if old[0] == "review" and new[0] == "review":
                        classification = "review"
                    elif old[0] in ("exact", "review"):
                        classification = "retained-pass" if new[0] == "exact" else "new-regression"
                    elif new[0] == "exact":
                        classification = "fixed"
                    elif new[0] == "review":
                        classification = "fixed-public-invariant"
                    else:
                        classification = "legacy-unfixed" if old == new else "changed-failure"
                    totals[classification] += 1
                    states[route][classification] += 1
                    if classification not in ("retained-pass", "review"):
                        changes[route] = dict(classification=classification, before=old, after=new)
                if changes:
                    differences.write(row_bytes(dict(sourceSha256=left["sourceSha256"], states=changes,
                                                       checkpoint=chunk.name, native=expected)))
    planned = json.loads((directory / "manifest.json").read_bytes())["sources"]
    summary = dict(planned=planned, paired=pairs, uncovered=planned - pairs, counts=dict(totals),
                   byRoute={route: dict(counts) for route, counts in states.items()})
    (directory / "classification.json").write_bytes(row_bytes(summary))
    return summary


def public_state(row, expected):
    if row is None:
        return ("product-error", None)
    if not all(row[key] for key in ("jsonEquivalent", "validDocument", "ansiEquivalent")):
        return ("public-invariant-failure", None)
    if expected is None:
        return ("review", None)
    try:
        actual = product_prose(row["plain"])
    except UnsupportedExtraction:
        return ("missing-description", None)
    return ("exact" if actual == expected["rows"] else "strict-row-difference", actual)
