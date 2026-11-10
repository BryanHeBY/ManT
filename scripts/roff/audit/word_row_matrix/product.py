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
from .compare import (
    STRICT, UnsupportedExtraction, group_witness, native_prose, product_prose, word_comparison,
)
from .fidelity import chain_obligations
from .manifest import jsonl, row_bytes
from .oracle import BATCH, atomic_output, check_checkpoint, stream_bytes
from .pending_observations import check_pending_rows
from .section_observations import check_section
from .section_padding_observations import check_section_padding
from .section_tail_observations import check_section_tail

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
    if "W-core" in source["cohorts"]:
        result["nativeWord"] = rewind_obligation(source, utf8, tree, observations.get("roff"))
    if any(cohort.startswith("SEC-") for cohort in source["cohorts"]):
        result["nativeSection"] = check_section(tree, utf8, observations.get("roff"))
    if any(cohort.startswith("PH-") for cohort in source["cohorts"]):
        result["nativePendingRow"] = check_pending_rows(source["source"], tree, utf8,
                                                        observations.get("roff"))
    if any(cohort.startswith("PHT-") for cohort in source["cohorts"]):
        result["nativePendingRow"] = check_section_tail(source["source"], tree, utf8,
                                                         observations.get("roff"))
    if any(cohort.startswith("PHD-") for cohort in source["cohorts"]):
        result["nativePendingRow"] = check_section_padding(source["source"], tree, utf8,
                                                            observations.get("roff"))
    return result


def rewind_obligation(source, receipt, tree, observation):
    if observation is None:
        return dict(state="blocked")
    if not all(observation[key] for key in ("jsonEquivalent", "validDocument", "ansiEquivalent")):
        return dict(state="public-invariant-failure")
    try:
        native = native_prose(receipt, tree)["rows"]
    except UnsupportedExtraction as error:
        return dict(state="review-extractor", reason=str(error))
    try:
        actual = product_prose(observation["plain"])
    except UnsupportedExtraction as error:
        return dict(state="difference", native=native, reason=str(error))
    state = word_comparison(source["source"], native, actual)
    return dict(state=state) if state == "exact" else dict(state=state, native=native, actual=actual)


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
                   profiles=list(ROUTES), verdictContract={
                       name: digest(Path(__file__).with_name(name).read_bytes())
                       for name in ("product.py", "compare.py", "followup.py", "section_observations.py",
                                    "pending_observations.py", "section_tail_observations.py",
                                    "section_padding_observations.py")
                   })
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
    native_word = Counter()
    native_section = Counter()
    native_pending = Counter()
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
            if obligation := row["verdict"].get("nativeWord"):
                native_word[obligation["state"]] += 1
            if obligation := row["verdict"].get("nativeSection"):
                native_section[obligation["state"]] += 1
            if obligation := row["verdict"].get("nativePendingRow"):
                native_pending[obligation["state"]] += 1
        (output / "progress.json").write_bytes(row_bytes(dict(
            completed=completed, routes=dict(routes), nativeWord=dict(native_word), nativeSection=dict(native_section), artifactBytes=size,
            wallSeconds=monotonic() - start)))
    planned = json.loads((directory / "manifest.json").read_bytes())["sources"]
    summary = dict(planned=planned, completed=completed, uncovered=planned - completed,
                   routes=dict(routes), nativeWord=dict(native_word), nativeSection=dict(native_section), artifactBytes=size,
                   wallSeconds=monotonic() - start)
    if native_pending:
        summary["nativePendingRow"] = dict(native_pending)
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
    fidelity_axes = {}
    review_reasons = Counter()
    section_axes = {}
    section_actions = Counter()
    pending_transitions = Counter()
    pending_axes = {}
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
                    if old[0] == "review":
                        classification = {
                            "review": "review",
                            "exact": "newly-asserted-pass",
                        }.get(new[0], "failure-without-baseline")
                    elif old[0] == "exact":
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
                fidelity_differences = {}
                fidelity_review = {}
                for route, stages in chain_obligations(right["observations"]).items():
                    for stage, axes in stages.items():
                        for axis, evidence in axes.items():
                            key = f"{route}/{stage}/{axis}"
                            fidelity_axes.setdefault(key, Counter())[evidence[0]] += 1
                            if evidence[0] == "difference":
                                fidelity_differences[key] = evidence[1:]
                            elif evidence[0] == "review":
                                reason = evidence[1]
                                fidelity_review.setdefault(reason, []).append(key)
                                review_reasons[reason] += 1
                section = right["verdict"].get("nativeSection")
                if section:
                    witness = section.get("witness", {})
                    if witness:
                        section_actions[(witness["node"]["label"], witness["action"])] += 1
                    for axis, evidence in section.get("axes", {}).items():
                        section_axes.setdefault(axis, Counter())[evidence[0]] += 1
                section_failed = section and section["state"] != "asserted-axes-pass"
                pending = right["verdict"].get("nativePendingRow")
                pending_before = left["verdict"].get("nativePendingRow")
                if pending:
                    pending_transitions[(pending_before["state"] if pending_before else "unclassified",
                                         pending["state"])] += 1
                    for axis, evidence in pending.get("axes", {}).items():
                        pending_axes.setdefault(axis, Counter())[evidence[0]] += 1
                pending_failed = pending and pending["state"] != "asserted-axes-pass"
                if changes or fidelity_differences or fidelity_review or section_failed or pending_failed:
                    differences.write(row_bytes(dict(
                        sourceSha256=left["sourceSha256"], states=changes,
                        fidelity=fidelity_differences, reviewAxes=fidelity_review,
                        section=section if section_failed else None,
                        **({"pendingRow": pending} if pending_failed else {}),
                        checkpoint=chunk.name, native=expected)))
    planned = json.loads((directory / "manifest.json").read_bytes())["sources"]
    summary = dict(planned=planned, paired=pairs, uncovered=planned - pairs, counts=dict(totals),
                   byRoute={route: dict(counts) for route, counts in states.items()},
                   readbackAxes={key: dict(counts) for key, counts in fidelity_axes.items()},
                   readbackReviewReasons=dict(review_reasons),
                   sectionAxes={key: dict(counts) for key, counts in section_axes.items()},
                   sectionActions={f"{macro}/{action}": count
                                   for (macro, action), count in sorted(section_actions.items())})
    if pending_transitions:
        summary["pendingRowTransitions"] = {f"{old}/{new}": count
                                            for (old, new), count in pending_transitions.items()}
        summary["pendingRowAxes"] = {key: dict(counts) for key, counts in pending_axes.items()}
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
