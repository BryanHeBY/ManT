"""Measure frozen CLI/operation artifacts; never build or change a checkout."""

import argparse
from collections import Counter
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import traceback

from .cards import CLI_MODES, FIXTURES, OPERATION_MODES, ROOT, build_card_admission, command, file_identity, fixed_input, host_card, measurement_scope, operation_harness_identity
from .statistics import RESAMPLES, SEED, paired_summary
from .transport import collect, resource_tool, validate_timeout


@dataclass(frozen=True)
class Plan:
    pairs_per_batch: int = 10
    warmups: int = 3
    aa_pairs: int = 10
    batches: int = 2
    seed: int = SEED
    resamples: int = RESAMPLES

    def __post_init__(self):
        if self.pairs_per_batch < 1 or self.warmups < 0 or self.aa_pairs < 2 or self.batches != 2 or self.resamples < 1:
            raise ValueError("positive pairs/resamples, nonnegative warmups, at least two A/A pairs and exactly two batches required")

    @property
    def formal_defaults(self):
        return self == Plan()

    def invocations(self):
        for round_index in range(self.warmups):
            for side in ("baseline", "candidate"):
                yield {"kind": "warmup", "pair": round_index, "side": side}
        before = self.aa_pairs // 2
        for placement, count, first in (("before", before, 0), ("after", self.aa_pairs - before, before)):
            if placement == "after":
                for batch in range(self.batches):
                    for pair in range(self.pairs_per_batch):
                        order = ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
                        for ordinal, side in enumerate(order):
                            yield {"kind": "paired", "batch": batch, "pair": pair, "order": "AB" if pair % 2 == 0 else "BA", "ordinal": ordinal, "side": side}
            for pair in range(first, first + count):
                for side in ("a1", "a2"):
                    yield {"kind": "aa", "placement": placement, "pair": pair, "side": side}


def summarize(records, plan):
    failures = [record for record in records if record["status"] != "ok"]
    expected = list(plan.invocations())
    plan_matches = len(records) == len(expected) and all(
        all(record.get(key) == value for key, value in item.items())
        for item, record in zip(expected, records)
    )
    complete = plan_matches and not failures
    pairs = [{index: {} for index in range(plan.pairs_per_batch)} for _ in range(plan.batches)]
    controls = {index: {} for index in range(plan.aa_pairs)}
    for record in records:
        if record["kind"] == "paired":
            pairs[record["batch"]][record["pair"]][record["side"]] = record
        elif record["kind"] == "aa":
            controls[record["pair"]][record["side"]] = record
    metrics = set().union(*(record["metrics"].keys() for record in records))
    summaries = {}
    for metric in sorted(metrics):
        groups = [[(pair.get("baseline"), pair.get("candidate")) for pair in batch.values()] for batch in pairs]
        aa = [(pair.get("a1"), pair.get("a2")) for pair in controls.values()]
        valid = all(left and right and left["status"] == right["status"] == "ok" and metric in left["metrics"] and metric in right["metrics"]
                    for group in [*groups, aa] for left, right in group)
        if not valid or not complete:
            summaries[metric] = {"status": "incomplete", "reason": "failed invocation, missing pair or missing metric; raw records preserved"}
            continue
        summaries[metric] = paired_summary(
            [[(left["metrics"][metric], right["metrics"][metric]) for left, right in batch] for batch in groups],
            [(left["metrics"][metric], right["metrics"][metric]) for left, right in aa], seed=plan.seed, resamples=plan.resamples)
    return {"status": "measured" if complete else "incomplete", "formalDefaultPlan": plan.formal_defaults,
            "invocationPlanMatches": plan_matches,
            "statuses": dict(Counter(record["status"] for record in records)), "metrics": summaries,
            "resourceMetrics": "whole-process resource observations, never operation-exclusive",
            "performanceAcceptance": "not decided by harness; correctness and allowed differences require independent review"}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def check_identity(record):
    """An absent or changed retained artifact is evidence drift, not a sample."""
    try:
        current = file_identity(record["path"])
        return {"drift": current != record, "current": current}
    except OSError as error:
        return {"drift": True, "error": str(error)}


def run_case(card, binaries, directory, plan, timeout, collector):
    directory.mkdir(parents=True)
    write_json(directory / "card.json", card)
    correctness = {}
    if card["panel"] == "cli":
        for side, binary in binaries.items():
            correctness[side] = collect(command(binary, card["input"], card["panel"], card["mode"]), directory,
                                        f"correctness-{side}", panel="cli", timeout=timeout, capture=True)
        write_json(directory / "correctness.json", {
            "runs": correctness,
            "classification": "unreviewed output evidence; hash equality or inequality is not a fidelity verdict",
        })
    records = []
    with (directory / "samples.jsonl").open("w") as ledger:
        for index, item in enumerate(plan.invocations()):
            binary = binaries["baseline" if item["kind"] == "aa" else item["side"]]
            record = {**item, "index": index, **collect(command(binary, card["input"], card["panel"], card["mode"]), directory,
                                                       f"{index:04}", panel=card["panel"], timeout=timeout, collector=collector)}
            ledger.write(json.dumps(record, allow_nan=False) + "\n")
            ledger.flush()
            records.append(record)
    result = summarize(records, plan)
    if correctness and any(record["status"] != "ok" for record in correctness.values()):
        result["status"] = "incomplete"
        result["correctnessFailure"] = True
    write_json(directory / "summary.json", result)
    return result


def parser():
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("--baseline", type=Path, required=True)
    args.add_argument("--candidate", type=Path, required=True)
    args.add_argument("--baseline-operations", type=Path)
    args.add_argument("--candidate-operations", type=Path)
    args.add_argument("--baseline-build-card", type=Path, required=True)
    args.add_argument("--candidate-build-card", type=Path, required=True)
    args.add_argument("--output", type=Path, required=True)
    args.add_argument("--panel", choices=["cli", "operation", "all"], default="all")
    args.add_argument("--pages", nargs="+", choices=FIXTURES, default=list(FIXTURES))
    args.add_argument("--modes", nargs="+", help="restrict modes, recorded as partial coverage")
    args.add_argument("--pairs", type=int, default=10, help="pairs per batch; 10 is the formal default")
    args.add_argument("--warmups", type=int, default=3)
    args.add_argument("--aa-pairs", type=int, default=10)
    args.add_argument("--timeout", type=float, default=120)
    return args


def run(args):
    validate_timeout(args.timeout)
    if args.modes and any(mode not in (*CLI_MODES, *OPERATION_MODES) for mode in args.modes):
        raise ValueError("unknown mode requested")
    plan = Plan(args.pairs, args.warmups, args.aa_pairs)
    args.output.mkdir(parents=True, exist_ok=False)
    binaries = {"cli": {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}}
    if args.panel != "cli":
        if not args.baseline_operations or not args.candidate_operations:
            raise ValueError("operation panel requires both frozen operation executables")
        binaries["operation"] = {"baseline": args.baseline_operations.resolve(), "candidate": args.candidate_operations.resolve()}
    input_directory = args.output / "inputs"
    input_directory.mkdir()
    inputs = [fixed_input(name, input_directory) for name in args.pages]
    identities = {panel: {side: file_identity(binary) for side, binary in sides.items()} for panel, sides in binaries.items()}
    collector = resource_tool()
    build_cards = {"baseline": json.loads(args.baseline_build_card.read_text()), "candidate": json.loads(args.candidate_build_card.read_text())}
    admission = {side: build_card_admission(build_cards[side], {panel: values[side] for panel, values in identities.items()})
                 for side in build_cards}
    manifest = {"schema": "mant.performance-measurement/v1", "started": datetime.now(timezone.utc).isoformat(),
                "harness": {str(path.relative_to(ROOT)): file_identity(path) for path in Path(__file__).parent.glob("*.py")},
                "plan": asdict(plan), "timeoutSeconds": args.timeout,
                "formalDefaultPlan": plan.formal_defaults, "host": host_card(), "inputs": inputs,
                "artifacts": identities, "buildCards": build_cards,
                "operationHarness": operation_harness_identity() if "operation" in binaries else None,
                "buildCardAdmission": admission,
                "complexityEvidence": {
                    "status": "not collected by runner; existing test-only proofs are separate from release wall and RSS",
                    "tests": ["growing_unbroken_suffix_is_scanned_incrementally",
                              "many_passes_in_one_operand_do_not_rescan_projection_prefixes",
                              "actual_head_post_uses_one_device_view_for_visible_invisible_and_pending_cells",
                              "accepted_owner_receipt_intersects_each_ordered_range_once",
                              "only_words_with_receipt_positions_build_cursor_history_and_text"],
                    "unmeasured": ["total allocation/clone bytes", "local parse/cache attempts", "whole-document index builds"],
                },
                "collector": collector, "resourceCoverage": "GNU time command rusage, including reaped child descendants; no detached-process tree claim" if collector else "unmeasured: no supported GNU time collector",
                "coverage": {"panel": args.panel, "pages": args.pages, "modes": args.modes, "correctness": "independent CLI evidence, unreviewed; no automatic semantic coverage claim"}}
    write_json(args.output / "manifest.json", manifest)
    if any(value["status"] != "bound" for value in admission.values()):
        write_json(args.output / "summary.json", {"cases": [], "status": "incomplete", "buildCardAdmission": admission})
        raise SystemExit(1)
    results = []
    for input_card in inputs:
        for panel in (["cli", "operation"] if args.panel == "all" else [args.panel]):
            available = CLI_MODES if panel == "cli" else OPERATION_MODES
            modes = [mode for mode in (args.modes or available) if mode in available]
            if not modes:
                raise ValueError(f"no valid {panel} modes selected")
            for mode in modes:
                card = {"schema": manifest["schema"], "input": input_card, "panel": panel, "mode": mode,
                        "scope": measurement_scope(panel, mode), "plan": manifest["plan"],
                        "timeoutSeconds": args.timeout, "buildCards": build_cards,
                        "artifacts": identities[panel], "outputDestination": "measurement JSON file" if panel == "operation" else os.devnull,
                        "allowedOutputDifferences": build_cards.get("candidate", {}).get("allowedOutputDifferences", "not recorded; separate correctness review required"),
                        "costAcceptance": build_cards.get("candidate", {}).get("costAcceptance", "not recorded; no automatic acceptance")}
                result = run_case(card, binaries[panel], args.output / f"{input_card['id']}-{panel}-{mode}", plan, args.timeout, collector)
                results.append({"id": f"{input_card['id']}-{panel}-{mode}", **result})
    artifact_checks = {panel: {side: check_identity(identity) for side, identity in values.items()}
                       for panel, values in identities.items()}
    input_checks = {card["id"]: {kind: check_identity(card[kind]) for kind in ("stored", "operationInput")}
                    for card in inputs}
    artifact_drift = any(check["drift"] for values in artifact_checks.values() for check in values.values())
    input_drift = any(check["drift"] for values in input_checks.values() for check in values.values())
    drift = artifact_drift or input_drift
    write_json(args.output / "summary.json", {"cases": results, "artifactDrift": artifact_drift,
                                             "inputDrift": input_drift,
                                             "identityChecks": {"artifacts": artifact_checks, "inputs": input_checks},
                                             "buildCardAdmission": admission,
                                             "status": "incomplete" if drift or any(case["status"] != "measured" for case in results) or any(value["status"] != "bound" for value in admission.values()) else "measured-unreviewed"})
    if drift or any(case["status"] != "measured" for case in results) or any(value["status"] != "bound" for value in admission.values()):
        raise SystemExit(1)


def main():
    interface = parser()
    args = interface.parse_args()
    if args.output.exists():
        interface.error("output must be a fresh directory; existing evidence is never overwritten")
    try:
        run(args)
    except Exception as error:
        if args.output.is_dir():
            write_json(args.output / "collection-error.json", {
                "status": "incomplete", "errorType": type(error).__name__,
                "error": str(error), "traceback": traceback.format_exc(),
            })
        raise


if __name__ == "__main__":
    main()
