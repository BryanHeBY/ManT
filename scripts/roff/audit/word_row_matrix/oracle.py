"""Checkpointed, exact-byte collection from the verified reference process."""
from base64 import b64decode, b64encode
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import gzip
from itertools import islice
import json
from pathlib import Path
from statistics import mean
import subprocess
from time import monotonic, time_ns

from scripts.roff.fixtures.reference_recipes import PROFILES, arguments, digest, environment
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference
from .manifest import jsonl, row_bytes

ROOT = Path(__file__).resolve().parents[4]
REFERENCE = ROOT / "target/mandoc-migration/reference/mandoc"
TIMEOUT = 15
BATCH = 128
BASE_PROFILES = [(profile, 160 if profile in ("ascii", "utf8") else None) for profile in PROFILES]
BASE_PROFILES.append(("utf8", 78))
EXTRA_PROFILES = [("ascii", 78), *((p, w) for w in (20, 40) for p in ("ascii", "utf8"))]


def stream_bytes(data):
    try:
        data.decode("utf-8")
        valid = True
    except UnicodeDecodeError:
        valid = False
    return dict(base64=b64encode(data).decode("ascii"), sha256=digest(data), utf8=valid)


def recipes():
    return [dict(profile=p, width=w, argv=[str(REFERENCE), *arguments(p, width=w or 160)],
                 environment=environment(), cwd=str(ROOT), timeoutSeconds=TIMEOUT)
            for p, w in BASE_PROFILES + EXTRA_PROFILES]


def raw_receipt(source, recipe):
    start = time_ns()
    try:
        result = run_reference(recipe["argv"][0], recipe["argv"][1:], input_bytes=source,
                               env=recipe["environment"], timeout=TIMEOUT)
        code, stdout, stderr = result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired as error:
        code, stdout, stderr = None, error.stdout or b"", error.stderr or b""
    return dict(code=code, startNs=start, wallNs=time_ns() - start,
                stdout=stream_bytes(stdout), stderr=stream_bytes(stderr))


def capture(record, catalog):
    source = record["source"].encode()
    if digest(source) != record["sourceSha256"]:
        raise ValueError("manifest source bytes changed")
    selected = catalog if record["extraWidths"] else catalog[:len(BASE_PROFILES)]
    return dict(sourceSha256=record["sourceSha256"],
                receipts={key: raw_receipt(source, recipe) for key, recipe in selected})


def check_checkpoint(source, result, catalog):
    if source["sourceSha256"] != result["sourceSha256"]:
        raise ValueError("source ordering changed for checkpoint")
    selected = catalog if source["extraWidths"] else catalog[:len(BASE_PROFILES)]
    if set(result["receipts"]) != {key for key, _ in selected}:
        raise ValueError("checkpoint does not contain every planned invocation")
    for receipt in result["receipts"].values():
        for name in ("stdout", "stderr"):
            raw = receipt[name]
            if digest(b64decode(raw["base64"], validate=True)) != raw["sha256"]:
                raise ValueError(f"cached {name} bytes changed")


@contextmanager
def atomic_output(path):
    scratch = path.with_suffix(path.suffix + ".partial")
    try:
        with gzip.GzipFile(scratch, "wb", compresslevel=3, mtime=0) as output:
            yield output
        scratch.replace(path)
    finally:
        scratch.unlink(missing_ok=True)


def collect(directory, *, jobs, wall_budget, artifact_budget, calibration=False, cohorts=()):
    if not 1 <= jobs <= 8:
        raise ValueError("native concurrency must be 1 to 8")
    if Path.cwd() != ROOT:
        raise ValueError("run the reference collector from the repository root")
    manifest_bytes = (directory / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    if digest((directory / "sources.jsonl.gz").read_bytes()) != manifest["inputs"]["sources.jsonl.gz"]:
        raise ValueError("source manifest integrity check failed")
    identity = verified_reference(ROOT, REFERENCE)["identity"]
    catalog = [(digest(row_bytes(recipe)), recipe) for recipe in recipes()]
    lane = "calibration" if calibration else "full" if not cohorts else "cohort-" + "-".join(cohorts)
    output = directory / "oracle" / lane
    output.mkdir(parents=True, exist_ok=True)
    binding = dict(manifestSha256=digest(manifest_bytes), oracleSha256=digest(REFERENCE.read_bytes()),
                   oracleIdentity=identity, recipes=dict(catalog))
    binding_path = output / "binding.json"
    if binding_path.exists() and json.loads(binding_path.read_bytes()) != binding:
        raise ValueError("oracle cache identity or invocation changed")
    binding_path.write_bytes(row_bytes(binding))
    budget = dict(jobs=jobs, perInvocationSeconds=TIMEOUT, wallSeconds=wall_budget,
                  artifactBytes=artifact_budget, maxSourceBytes=131072,
                  checkpointSources=BATCH, cacheDirectory=str(output.resolve()))
    (output / "budget.json").write_bytes(row_bytes(budget))
    start = monotonic()
    completed = invocations = cached = timeouts = 0
    initial_size = sum(p.stat().st_size for p in output.glob("*.jsonl.gz"))
    size = initial_size
    timings = {}
    planned = 0
    stopping = None
    stream = (record for record in jsonl(directory / "sources.jsonl.gz")
              if (not calibration or record["calibration"])
              and (not cohorts or not set(cohorts).isdisjoint(record["cohorts"])))
    with ThreadPoolExecutor(max_workers=jobs) as executor:
        for index, chunk in enumerate(iter(lambda: list(islice(stream, BATCH)), [])):
            planned += len(chunk)
            if stopping:
                continue
            path = output / f"{index:06}.jsonl.gz"
            cache_hit = path.exists()
            if cache_hit:
                results = jsonl(path)
                cached += len(chunk)
            elif monotonic() - start >= wall_budget or size >= artifact_budget:
                stopping = "wall-or-artifact-budget"
                continue
            else:
                if any(len(row["source"].encode()) > budget["maxSourceBytes"] for row in chunk):
                    raise ValueError("source exceeds configured budget")
                with atomic_output(path) as writer:
                    for result in executor.map(lambda record: capture(record, catalog), chunk):
                        writer.write(row_bytes(result))
                size += path.stat().st_size
                results = jsonl(path)
            for source, result in zip(chunk, results, strict=True):
                if cache_hit:
                    check_checkpoint(source, result, catalog)
                completed += 1
                for key, receipt in result["receipts"].items():
                    invocations += 1
                    timeouts += receipt["code"] is None
                    if calibration:
                        timings.setdefault(key, []).append(receipt["wallNs"] / 1e9)
            progress = dict(completed=completed, invocations=invocations, cached=cached,
                            timeouts=timeouts, artifactBytes=size, wallSeconds=monotonic() - start)
            (output / "progress.json").write_bytes(row_bytes(progress))
    summary = dict(completed=completed, invocations=invocations, cached=cached,
                   timeouts=timeouts, artifactBytes=size, wallSeconds=monotonic() - start,
                   planned=planned, uncovered=planned - completed, stop=stopping,
                   addedArtifactBytes=size - initial_size)
    if calibration:
        summary["perRecipe"] = {
            f"{recipe['profile']}/{recipe['width']}": dict(
                count=len(timings[key]), meanSeconds=mean(timings[key]),
                p95Seconds=sorted(timings[key])[int((len(timings[key]) - 1) * .95)])
            for key, recipe in catalog if key in timings
        }
    (output / "summary.json").write_bytes(row_bytes(summary))
    return summary
