#!/usr/bin/env python3
"""Replay finite rule boundaries without collapsing rows or diagnostic inputs.

Sources come from permanent generators; all five pristine profiles precede
candidate replay. Case identities and distinct source counts remain separate.
Raw differences, qualified failures and uncovered dimensions have independent
totals. A policy must bind the complete source and pristine output hashes.
"""

import argparse
import collections
import concurrent.futures
import hashlib
import gzip
import json
from pathlib import Path

from scripts.roff.fixtures import acceptance_comparison as comparison
from scripts.roff.fixtures import acceptance_regions as regions
from scripts.roff.fixtures import escape_rule_cases
from scripts.roff.fixtures import escape_projection_policies
from scripts.roff.fixtures import field_projection_policies
from scripts.roff.fixtures import integrity_rule_cases
from scripts.roff.fixtures import markdown_rule_cases
from scripts.roff.fixtures.markdown_reader_observer import markdown_reader_axes
from scripts.roff.fixtures import replay_roff_acceptance as transport
from scripts.roff.fixtures import roff_fixture_reference
from scripts.roff.fixtures import reference_recipes
from scripts.roff.fixtures import rule_boundary_cases
from scripts.roff.fixtures import rule_closure_fields

ROOT = Path(__file__).resolve().parents[3]
COHORTS = {
    "retained-review": rule_boundary_cases,
    "escape": escape_rule_cases,
    "field": rule_closure_fields,
    "markdown": markdown_rule_cases,
    "integrity": integrity_rule_cases,
}
STRUCTURAL_AXES = ("content", "rows", "separators")
SCHEMA = "mant.rule-boundary-replay/v1"
PROFILES = ("ascii", "utf8", "html", "tree", "lint")
PROJECTION_CARDS = ROOT / "tests/fixtures/roff/rule_boundaries/projection_cards.json.gz"
RECOVERY_CARDS = ROOT / "tests/fixtures/roff/rule_boundaries/source_recovery_cards.json.gz"
INTEGRITY_CARDS = ROOT / "tests/fixtures/roff/rule_boundaries/integrity_projection_cards.json"


def sha(data):
    if isinstance(data, str):
        data = data.encode()
    return hashlib.sha256(data).hexdigest()


def load_bound_cards(path):
    result = {}
    raw = gzip.decompress(path.read_bytes()) if path.suffix == ".gz" else path.read_bytes()
    for card in json.loads(raw):
        if card["id"] in result:
            raise ValueError("duplicate bound card identity: " + card["id"])
        result[card["id"]] = card
    return result


def assemble(cohorts=None):
    """Admit no duplicate identity and validate supplied source hashes."""
    result, identities = [], set()
    for cohort in cohorts or COHORTS:
        for generated in COHORTS[cohort].cases():
            one = dict(generated, cohort=cohort)
            if one["id"] in identities:
                raise ValueError(f"duplicate execution identity: {one['id']}")
            identities.add(one["id"])
            actual = sha(one["source"])
            if one.get("source_sha256", actual) != actual:
                raise ValueError(f"source hash changed: {one['id']}")
            one["source_sha256"] = actual
            one.setdefault("metadata", {})
            result.append(one)
    return result


def generation_plan(cases):
    counts = collections.Counter(one["cohort"] for one in cases)
    rules = collections.Counter()
    pairs = collections.defaultdict(set)
    for one in cases:
        labels = one.get("rule_id", "unassigned")
        for rule in labels if isinstance(labels, list) else [labels]:
            rules[rule] += 1
        for key, value in one["metadata"].items():
            if key.startswith("pair"):
                pairs[key].add(json.dumps(value, sort_keys=True))
    return {
        "schema": SCHEMA, "identities": len(cases),
        "unique_sources": len({one["source_sha256"] for one in cases}),
        "cohorts": dict(counts), "rules": dict(rules),
        "declared_pair_keys": {key: len(values) for key, values in pairs.items()},
        "ordered_id_source_sha256": sha(json.dumps([
            [one["id"], one["source_sha256"]] for one in cases])),
        "scope": "pair keys are declarations; AST reachability is a separate assertion",
    }


def admission(oracle):
    """Diagnostics do not authorize content loss or skipped assertions."""
    if any(not reference_recipes.completed(oracle[profile], 2)
           for profile in ("ascii", "utf8", "html", "tree")):
        return "generator-invalid"
    code = oracle["lint"]["code"]
    if not reference_recipes.completed(oracle["lint"]):
        return "generator-invalid"
    if code == 0:
        return "valid"
    return "diagnosed-valid" if code <= 2 else "recovery"


def validate_oracle_record(one, oracle):
    reference_recipes.validate_streams(one["source_sha256"], oracle)


def validated_cache(evidence, binding, cases, *, allow_missing=False, collection_reference=None):
    directory, path, cache = transport.load_cache(
        evidence, binding["identity"], binding["reference_sha256"],
        collection_reference=collection_reference)
    for one in cases:
        if reference_recipes.key(one) in cache:
            validate_oracle_record(one, cache[reference_recipes.key(one)])
        elif not allow_missing:
            raise ValueError("missing pristine record: " + one["id"])
    return directory, path, cache


def collect_reference(reference, registration, cases, evidence, workers):
    """Keep rejected, timed-out and invalid-UTF8 profiles in the ledger."""
    binding = {"identity": registration["identity"], "reference_sha256": sha(reference.read_bytes())}
    directory, path, cache = validated_cache(
        evidence, binding, cases, allow_missing=True, collection_reference=reference)
    profiles = PROFILES
    unique = {reference_recipes.key(one): one for one in cases}
    missing = [one for digest, one in unique.items()
               if digest not in cache or not all(name in cache[digest] for name in profiles)]
    directory.mkdir(parents=True, exist_ok=True)
    print(f"oracle cache: {len(cache)} cached, {len(missing)} to collect", flush=True)

    with path.open("a") as output:
        with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
            for index, result in enumerate(pool.map(
                    lambda one: transport.record_oracle(reference, one), missing), 1):
                output.write(json.dumps(result, ensure_ascii=False) + "\n")
                if index % 500 == 0:
                    print(f"collected {index}/{len(missing)}", flush=True)
    (directory / "manifest.json").write_text(json.dumps({
        "identity": registration["identity"], "reference_sha256": sha(reference.read_bytes()),
        "reference_path": str(reference.resolve()),
        "records": len(set(cache) | set(unique)), "expectations_from_product": False,
        **reference_recipes.cache_binding(),
    }, indent=2) + "\n")


def coverage_expected(one):
    metadata = one["metadata"]
    if "native_guard_expected" in metadata:
        return metadata["native_guard_expected"]
    if one["family"] == "depth":
        return metadata["depth"] > 256
    if one["cohort"] == "escape" and one.get("rule_id") == "E07":
        return metadata.get("depth", 0) > 256
    return None


def execute(product, one):
    """One transport per distinct source; preserve every raw result."""
    source = one["source"]
    raw = {
        "plain": transport.run(product, transport.PRODUCT_TEXT, source),
        "ansi": transport.run(product, transport.PRODUCT_ANSI, source),
        "json": transport.run(product, transport.PRODUCT_JSON, source),
    }
    bundle, error = None, None
    try:
        bundle = json.loads(raw["json"]["stdout"])
        if not isinstance(bundle, dict) or not isinstance(bundle.get("document"), dict):
            raise ValueError("JSON has no document owner")
    except (ValueError, TypeError) as exception:
        error = str(exception)
        bundle = None
    projection, targets, findings = {"status": "uncovered", "reason": "invalid product JSON"}, [], []
    if bundle is not None:
        try:
            projection = regions.select_product_region(raw["plain"]["stdout"], bundle)
            targets = comparison.product_external_targets(bundle)
            findings = [finding for finding in bundle["document"].get("diagnostics", [])
                        if finding.get("impact") == "content-coverage"]
        except (ValueError, TypeError, AttributeError, KeyError) as exception:
            error = str(exception)
    result = {
        "product_error": error is not None or any(
            profile["code"] != 0 or not profile["utf8_valid"]
            or profile["timeout"] for profile in raw.values()),
        "json_error": error,
        "ansi_parity": transport.unstyle(raw["plain"]["stdout"])
                       == transport.unstyle(raw["ansi"]["stdout"]),
        "marker_leak": "\\u0000mant:" in raw["json"]["stdout"],
        "region": projection,
        "targets": targets,
        "coverage_diagnostics": findings,
        "bundle": bundle,
    }
    if any(alias["cohort"] == "markdown" or alias["family"] == "consumer"
           for alias in one["aliases"]):
        markdown = transport.run(product, transport.PRODUCT_MARKDOWN, source)
        readback = transport.run(product, ["--input", "-", "--input-format", "markdown",
                                "--display", "direct", "--format", "text", "--color", "never"], markdown["stdout"])
        readback_json = transport.run(product, ["--input", "-", "--input-format", "markdown",
                                     "--format", "json", "--compact"], markdown["stdout"])
        raw.update(markdown=markdown, readback=readback, readback_json=readback_json)
        try:
            reader_bundle = json.loads(readback_json["stdout"])
            result["reader_region"] = regions.select_product_region(readback["stdout"], reader_bundle)
            result["reader_targets"] = comparison.product_external_targets(reader_bundle)
            result["reader_bundle"] = reader_bundle
            result["reader_tag_leak"] = any(
                marker in readback["stdout"] for marker in ("<br>", "<br />"))
        except (ValueError, TypeError, AttributeError):
            result["product_error"] = True
        result["product_error"] |= any(profile["code"] != 0 or profile["timeout"]
                                        or not profile["utf8_valid"] for profile in raw.values())
    return result, raw


def evaluate(one, oracle, product, binding, policies):
    """Return independent axes; missing evidence never becomes a pass."""
    validate_oracle_record(one, oracle)
    native = regions.select_native_region(oracle["utf8"]["stdout"], oracle["tree"]["stdout"])
    entry = policies.get(one["id"])
    field_witness = None
    retained_witness = None
    if one["cohort"] == "retained-review" and one["family"] in ("field-transition", "ti-transition"):
        expected_owner = "It BODY" if one["metadata"].get("kind") == "column" else "It HEAD"
        observation = dict(one, metadata=dict(one["metadata"], expected_owner=expected_owner))
        retained_witness = field_projection_policies.owner_witness(observation, oracle["tree"]["stdout"])
    device_card = one["cohort"] == "field" or bool(entry and entry.get("type") == "device-projection")
    if device_card:
        policy, invalid_policy = field_projection_policies.qualified_policy(one, oracle, binding, policies)
        if one["cohort"] == "field":
            field_witness = field_projection_policies.owner_witness(one, oracle["tree"]["stdout"])
    elif entry and entry.get("rule") == "numbered-source-spelling-recovery":
        policy, invalid_policy = escape_projection_policies.qualified_policy(one, oracle, binding, entry)
        policy = policy or {}
    else:
        policy, invalid_policy = comparison.qualified_policy(one, oracle, binding, policies)
    if policy and policy.get("id") != one["id"]:
        policy, invalid_policy = {}, "policy execution identity changed"
    raw_axes = comparison.compare_axes(native, product["region"])
    if device_card and not invalid_policy:
        qualified_native = regions.select_native_region(
            oracle["utf8"]["stdout"], oracle["tree"]["stdout"], margin_policy=policy.get("margin_policy"))
        qualified_native, qualified_product = field_projection_policies.project_regions(
            qualified_native, product["region"], policy)
        report = comparison.compare_axes(qualified_native, qualified_product)
        if field_witness:
            report["ast-reachability"] = field_witness["reachable"]
    else:
        report = comparison.compare_axes(native, product["region"], policy)
    expected_rows = policy.get("expected_product_rows", policy.get("expected_product_readable_rows"))
    if expected_rows is not None:
        # Explicit source fallback is an independent documented reading
        # contract, rather than a family-wide native text normalization.
        expected = {"status": "asserted", "rows": expected_rows}
        report = comparison.compare_axes(expected, product["region"])
    targets = comparison.native_external_targets(oracle["html"]["stdout"])
    report["link-identity"] = targets == product["targets"]
    expected_coverage = coverage_expected(one)
    if expected_coverage is not None:
        report["content-coverage"] = bool(product["coverage_diagnostics"]) == expected_coverage
        if expected_coverage:
            # Safety-protected input is intentionally not a promise of full
            # pristine text parity. Check the accepted independent witnesses
            # and actual omission receipt; keep unproved row geometry open.
            readable = "\n".join(product["region"].get("rows", []))
            prefix = one["metadata"].get("prefix", "A")
            after = readable.rfind("AFTER")
            prefix_position = readable.find(prefix)
            report.update(content="bounded-omission", rows="uncovered", separators="uncovered",
                          accepted_prefix=0 <= prefix_position < after,
                          later_independent=after >= 0)
    if "reader_region" in product:
        reader = markdown_reader_axes(one, product["bundle"], product.get("reader_bundle"))
        report.update({"markdown-reader-" + axis: value for axis, value in reader.items()
                       if axis in (*STRUCTURAL_AXES, "container", "body-ownership", "source-binding")})
        report["markdown-reader-tag-leak"] = not product["reader_tag_leak"]
        # Fenced column/literal fallbacks are intentionally plain text. The
        # missing active identity is exposed as uncovered, never waived.
        container = one["metadata"].get("container", one["metadata"].get("context"))
        report["markdown-reader-link-identity"] = ("uncovered" if container in ("column", "literal")
                                                   else product["targets"] == product["reader_targets"])
    if invalid_policy:
        report["policy-binding"] = False
    source_class = admission(oracle)
    if source_class == "generator-invalid":
        # Partial/crashed oracle output supplies no product gold. Keep raw
        # comparisons and process evidence while withholding that conclusion.
        report = comparison.qualified_axes(source_class, report)
    required = set(one.get("axes", []))
    required.update(one["metadata"].get("required_axes", []))
    required.update(one["metadata"].get("expected_axes", []))
    required.update(policy.get("uncovered", []))
    required.update(("style", "source", "scalar-range", "query", "tui"))
    aliases = {"identity": "link-identity", "later-line": "later_independent"}
    for axis in sorted(required):
        executed = aliases.get(axis, axis)
        if axis == "json":
            report[axis] = not product["product_error"] if source_class != "generator-invalid" else "uncovered"
        else:
            report.setdefault(axis, report.get(executed, "uncovered"))
    missing = [axis for axis, value in report.items() if value == "uncovered"]
    execution = product if source_class != "generator-invalid" else {}
    verdict, failures = comparison.verdict(source_class, report, execution, missing)
    return {
        "id": one["id"], "cohort": one["cohort"], "family": one["family"],
        "rule_id": one.get("rule_id"), "source_sha256": one["source_sha256"],
        "source_class": source_class, "metadata": one["metadata"],
        "native_region": native, "product_region": product["region"],
        "raw_axes": raw_axes, "axes": report,
        "policy_id": policy.get("id"), "policy_error": invalid_policy,
        "verdict": verdict, "failures": failures, "uncovered": missing,
        "product_execution_error": product["product_error"],
        "ast_witness": field_witness,
        "retained_ast_witness": retained_witness,
        "generator_scope": ("unreachable-declared-head" if retained_witness and not retained_witness["reachable"]
                            else "reachable" if retained_witness else "not-observed"),
        "profile_status": {name: profile["code"] for name, profile in oracle.items()
                           if isinstance(profile, dict) and "code" in profile},
        "profile_sha256": {name: profile["stdout_sha256"] for name, profile in oracle.items()
                           if isinstance(profile, dict) and "stdout_sha256" in profile},
    }


def summarize(records):
    """Count aliases and unique sources without conflating raw and qualified."""
    cohorts = {}
    for record in records:
        total = cohorts.setdefault(record["cohort"], collections.Counter())
        total["identities"] += 1
        total[record["verdict"]] += 1
        total["raw_axis_differences"] += any(record["raw_axes"].get(axis) is False
                                             for axis in STRUCTURAL_AXES)
        total["unextractable"] += any(record["raw_axes"].get(axis) == "uncovered"
                                     for axis in STRUCTURAL_AXES)
        total["markdown_reader_tag_leak"] += record["axes"].get("markdown-reader-tag-leak") is False
        total["product_execution_errors"] += bool(record.get("product_execution_error"))
    return {
        "schema": SCHEMA, "identities": len(records),
        "unique_sources": len({record["source_sha256"] for record in records}),
        "verdicts": dict(collections.Counter(record["verdict"] for record in records)),
        "sources_with_failures": len({record["source_sha256"] for record in records if record["failures"]}),
        "product_execution_error_identities": sum(bool(record.get("product_execution_error")) for record in records),
        "product_execution_error_sources": len({record["source_sha256"] for record in records
                                                if record.get("product_execution_error")}),
        "failures_by_axis": dict(collections.Counter(axis for record in records for axis in record["failures"])),
        "source_classes": dict(collections.Counter(record["source_class"] for record in records)),
        "cohorts": cohorts,
        "qualification": "raw native differences are not automatically product defects; uncovered consumers stay review",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cohort", action="append", choices=COHORTS)
    parser.add_argument("--check-generation", action="store_true")
    parser.add_argument("--collect", action="store_true")
    parser.add_argument("--replay", action="store_true")
    parser.add_argument("--product", type=Path, default=ROOT / "target/debug/mant")
    parser.add_argument("--product-head")
    parser.add_argument("--reference", type=Path, default=transport.REFERENCE)
    parser.add_argument("--evidence", type=Path, default=ROOT / "target/audits/rule-boundaries")
    parser.add_argument("--oracle-evidence", type=Path)
    parser.add_argument("--policy", action="append", type=Path, default=[])
    parser.add_argument("--workers", type=int, default=4)
    args = parser.parse_args()
    cases = assemble(args.cohort)
    plan = generation_plan(cases)
    if args.check_generation:
        print(json.dumps(plan, indent=2))
        if not args.collect and not args.replay:
            return 0
    registration = roff_fixture_reference.verified_reference(ROOT, args.reference)
    args.evidence.mkdir(parents=True, exist_ok=True)
    oracle_evidence = args.oracle_evidence or args.evidence
    (args.evidence / "plan.json").write_text(json.dumps(plan, indent=2) + "\n")
    if args.collect:
        collect_reference(args.reference, registration, cases, oracle_evidence, args.workers)
    oracle_binding = {"identity": registration["identity"], "reference_sha256": sha(args.reference.read_bytes())}
    _, cache_path, cache = validated_cache(oracle_evidence, oracle_binding, cases, allow_missing=not args.replay)
    if not args.replay:
        return 0
    missing = [one["id"] for one in cases if reference_recipes.key(one) not in cache]
    if missing:
        raise SystemExit(f"missing pristine records: {len(missing)}; collect before replay")
    policies = escape_projection_policies.load_policies()
    policies.update(field_projection_policies.load_policies())
    for path in (PROJECTION_CARDS, RECOVERY_CARDS, INTEGRITY_CARDS):
        for key, card in load_bound_cards(path).items():
            if key in policies:
                raise SystemExit("duplicate bound card identity: " + key)
            policies[key] = card
    for path in args.policy:
        for key, policy in comparison.load_policies(path).items():
            if key in policies:
                raise SystemExit(f"duplicate policy identity: {key}")
            policies[key] = policy
    product_binding = transport.product_binding(args.product, args.product_head)
    unique = {}
    for one in cases:
        record = unique.setdefault(one["source_sha256"], dict(one, aliases=[]))
        record["aliases"].append(one)
    outputs = {}
    with (args.evidence / "product-raw.jsonl").open("w") as output:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
            for index, (one, (result, raw)) in enumerate(zip(
                    unique.values(), pool.map(lambda case: execute(args.product, case), unique.values())), 1):
                outputs[one["source_sha256"]] = result
                output.write(json.dumps({"source_sha256": one["source_sha256"], "profiles": raw}, ensure_ascii=False) + "\n")
                if index % 500 == 0:
                    print(f"replayed {index}/{len(unique)}", flush=True)
    records = [evaluate(one, cache[reference_recipes.key(one)], outputs[one["source_sha256"]],
                        oracle_binding, policies) for one in cases]
    if sha(args.product.read_bytes()) != product_binding["sha256"]:
        raise SystemExit("candidate binary changed during replay")
    with (args.evidence / "ledger.jsonl").open("w") as output:
        for record in records:
            output.write(json.dumps(record, ensure_ascii=False) + "\n")
    summary = summarize(records)
    summary.update(oracle_binding=oracle_binding, product_binding=product_binding,
                   generation=plan, ledger_sha256=sha((args.evidence / "ledger.jsonl").read_bytes()),
                   raw_sha256=sha((args.evidence / "product-raw.jsonl").read_bytes()),
                   oracle_cache_sha256=sha(cache_path.read_bytes()))
    (args.evidence / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    return int(summary["verdicts"].get("fail", 0) != 0)


if __name__ == "__main__":
    raise SystemExit(main())
