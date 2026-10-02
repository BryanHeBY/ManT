#!/usr/bin/env python3
"""Replay the frozen shared-execution acceptance matrix (guide sections 5.6/5.8).

Layered execution, mirroring section 5.8:

1. Oracle acquisition layer.  Every distinct complete source is rendered by
   the registered pristine oracle exactly once per (source hash, oracle
   identity, command); results live in a cache below ``target``.  Changing
   the source, the pinned oracle identity or the projection policy version
   invalidates the cache.  Product output never enters it.
2. Fast shared execution layer.  All cases replay against one product binary
   inside this single process run; every failing case id is aggregated, the
   recorder never aborts at the first failure.
3. Admission ledger.  ``lint`` plus the pristine ``tree`` qualify each case
   as legal / native-diagnostics / recovery / not-applicable /
   generator-defect before any expectation is frozen.  Only the frozen
   selection (family representatives, RR anchors and directed RR-adjacent
   pairs declared in ``roff_acceptance_cases``) is checked in, with its full
   five-profile oracle record; the complete matrix stays replay-only.

Interface for the axis-assertion consumers (comparator module): the ledger
schema below plus ``scripts/roff/fixtures/acceptance/oracle/<id>.json``
records provide, per case, the exact source, source hash, axis values,
effective axis policy (section 5.7 vocabulary), admission class and the
pristine oracle evidence.  Consumers add assertions on top; they never
rewrite gold (``--check-frozen`` verifies the checked-in records).

Examples:

    python3 scripts/_run_module.py scripts.roff.fixtures.replay_roff_acceptance \\
        --check-generation
    python3 scripts/_run_module.py scripts.roff.fixtures.replay_roff_acceptance \\
        --collect --replay --ledger --product path/to/mant
    python3 scripts/_run_module.py scripts.roff.fixtures.replay_roff_acceptance \\
        --freeze --product path/to/mant
"""

import argparse
import collections
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from scripts.roff.fixtures import roff_acceptance_cases
from scripts.roff.fixtures import roff_execution_cases
from scripts.roff.fixtures import roff_fixture_reference
from scripts.roff.fixtures import reference_recipes
from scripts.roff.fixtures import acceptance_comparison
from scripts.roff.fixtures import acceptance_regions
from scripts.roff.lib.roff_content_compare import visible_text

ROOT = Path(__file__).resolve().parents[3]
REFERENCE = ROOT / "target/mandoc-migration/reference/mandoc"
ACCEPTANCE = ROOT / "scripts/roff/fixtures/acceptance"
LEDGER_SCHEMA = "mant.roff-acceptance-ledger/v2"
POLICY_VERSION = 2
# Fixed page-furniture marker for end-of-file tail projections: the footer
# date row always carries the frozen header date of these generated sources.
DATE_TOKEN = "September 28, 2026"


def digest(data):
    return hashlib.sha256(data).hexdigest()


# ---------------------------------------------------------------------------
# Case assembly (frozen definitions only; deterministic and idempotent)
# ---------------------------------------------------------------------------

def all_cases():
    """Return the full acceptance case list with stable ids and cohorts."""
    cases = []
    for matrix, matrix_cases in roff_execution_cases.matrices():
        if matrix != "formatter":
            continue
        for index, one in enumerate(matrix_cases):
            cases.append({"cohort": "replay", "id": f"formatter-{index:04}", **one})
    for cohort, family, family_cases in roff_acceptance_cases.families():
        for index, one in enumerate(family_cases):
            cases.append({"cohort": cohort, "id": f"{family}-{index:04}", **one})
    for one in cases:
        one["source_sha256"] = digest(one["source"].encode())
        one["policy"] = roff_acceptance_cases.axis_policy(one) \
            if one["cohort"] != "replay" else replay_cohort_policy(one)
    return cases


def replay_cohort_policy(one):
    """Section 5.7 policy for the historical replay cohort (A-L sets)."""
    context = one.get("context")
    rows = {
        "filled": "constrained-events: filled reflow, only authored breaks are hard rows",
        "nf": "exact-hard-rows",
        "literal": "exact-hard-rows",
        "unfilled": "exact-hard-rows",
        "EX": "exact-hard-rows",
        "tag": "exact-hard-rows",
        "hang": "exact-hard-rows",
        "column": "exact-hard-rows",
    }.get(context, "exact-hard-rows")
    return {
        "content": "exact",
        "separators": "constrained-events",
        "rows": rows,
        "indent": "omit-common-margin",
        "identity": "rich-inline" if "Lk" in one.get("source", "") else
                    "display-and-safety",
    }


def select_frozen(cases):
    """Resolve the frozen selection names to concrete case ids."""
    by_family = collections.defaultdict(list)
    for one in cases:
        by_family[one["family"]].append(one)
    resolved = {}
    for name, family, axes, role, note in roff_acceptance_cases.FROZEN_SELECTIONS:
        matches = [c for c in by_family[family]
                   if all(c.get(key) == value for key, value in axes.items())]
        if len(matches) != 1:
            raise ValueError(f"frozen selection {name}: {len(matches)} matches")
        resolved[name] = {"case": matches[0], "role": role, "note": note}
    return resolved


# ---------------------------------------------------------------------------
# Shared transport and screening projections (historical probe semantics)
# ---------------------------------------------------------------------------

def run(binary, arguments, source, timeout=20):
    try:
        completed = subprocess.run(
            [str(binary), *arguments], input=source.encode(), capture_output=True,
            timeout=timeout, env=reference_recipes.environment(), check=False)
        code, stdout, stderr = completed.returncode, completed.stdout, completed.stderr
        timed_out = False
    except subprocess.TimeoutExpired as error:
        code, stdout, stderr = None, error.stdout or b"", error.stderr or b""
        timed_out = True
    result = {"code": code, "timeout": timed_out, "utf8_valid": True}
    for name, data in (("stdout", stdout), ("stderr", stderr)):
        try:
            result[name] = data.decode("utf-8")
        except UnicodeDecodeError:
            # A partial timed-out write or corrupt internal UTF-8 is evidence,
            # not silently repaired text. Keep bytes and fail the execution.
            result["utf8_valid"] = False
            result[name] = data.decode("utf-8", "replace")
            result[name + "_bytes_hex"] = data.hex()
    return result


def unstyle(text):
    # Reuse the bounded display scanner: only known SGR and actual writes
    # over a preceding backspace are styling. Unknown controls stay visible.
    return visible_text(text)


def body_rows(text, terminal, *, eof_tail=False):
    """Screening row projection between DESCRIPTION and the tail boundary.

    Screening-only projection (historical probe semantics): interior blank
    rows are kept; the blank edges of the extracted window are removed.  For
    end-of-file tails there is no trailing sentinel, so the fixed page footer
    (blank row, optional OS row, date row carrying the frozen header date) is
    removed by cutting at the last blank row before the date row.  Exact
    edge-row assertion stays an uncovered axis for those cases.
    """
    rows = unstyle(text).splitlines()
    if "DESCRIPTION" not in rows:
        return None
    start = rows.index("DESCRIPTION") + 1
    if eof_tail:
        rows = rows[start:]
        dates = [i for i, row in enumerate(rows) if DATE_TOKEN in row]
        if dates:
            blanks = [i for i in range(dates[-1]) if rows[i] == ""]
            rows = rows[:blanks[-1] if blanks else dates[-1]]
    elif terminal in rows:
        rows = rows[start:rows.index(terminal)]
    else:
        return None
    while rows and not rows[0].strip():
        rows.pop(0)
    while rows and not rows[-1].strip():
        rows.pop()
    return rows


def screening(native_rows, product_rows):
    """Glyph/row/indent screening equalities; screening never proves axes."""
    if native_rows is None or product_rows is None:
        return {"extraction": "not-applicable"}
    return {
        "extraction": "ok",
        "glyph_equal": ("".join("".join(native_rows).split())
                        == "".join("".join(product_rows).split())),
        "row_equal": ([" ".join(row.split()) for row in native_rows]
                      == [" ".join(row.split()) for row in product_rows]),
        "indent_equal": ([row.lstrip() for row in native_rows]
                         == [row.lstrip() for row in product_rows]),
    }


# ---------------------------------------------------------------------------
# Layer 1: oracle acquisition with source-hash cache
# ---------------------------------------------------------------------------

# A tree text node is "<content> (text) [*]line:col [FLAGS...]"; the content
# may be empty and the position may carry an asterisk or trailing node flags
# such as NOFILL/NOPRT, all of which belong to the position, not the content.
TEXT_NODE = re.compile(r"^(.*?)\s*\(text\) \*?(\d+):(\d+)\s*(?:[A-Z]+\s*)*$")
NODE_KIND = re.compile(r"^\s*(\w+) \((?:block|elem)\)")


def tree_witness(tree_stdout):
    """Compact structural witness extracted from the pristine tree."""
    witness = {"text_nodes": 0, "empty_text_nodes": 0, "broken_flags": 0,
               "it_body_groups": 0, "text_positions": [], "kinds": {}}
    kinds = collections.Counter()
    for line in tree_stdout.splitlines():
        match = TEXT_NODE.match(line.strip())
        if match:
            witness["text_nodes"] += 1
            if not match.group(1).strip():
                witness["empty_text_nodes"] += 1
            witness["text_positions"].append(
                [int(match.group(2)), int(match.group(3))])
        if "BROKEN" in line:
            witness["broken_flags"] += 1
        if "It (body)" in line:
            witness["it_body_groups"] += 1
        kind = NODE_KIND.match(line)
        if kind:
            kinds[kind.group(1)] += 1
    witness["kinds"] = dict(sorted(kinds.items()))
    witness["text_positions"] = witness["text_positions"][:64]
    return witness


def lint_classes(lint):
    """Classify pristine lint output by severity class."""
    counts = collections.Counter()
    for message in (lint["stdout"] + lint["stderr"]).splitlines():
        for severity in ("STYLE", "WARNING", "ERROR", "FATAL"):
            if severity in message:
                counts[severity] += 1
                break
    return dict(sorted(counts.items()))


def load_cache(evidence, identity, reference_sha256=None, *, collection_reference=None):
    cache_dir = evidence / f"oracle-cache-{re.sub(r'[^A-Za-z0-9_.-]', '_', identity)}"
    cache = {}
    path = cache_dir / "cache.jsonl"
    if path.exists():
        manifest = json.loads((cache_dir / "manifest.json").read_bytes())
        reference_recipes.validate_manifest(
            manifest, identity, reference_sha256)
        if (collection_reference is not None
                and manifest["reference_path"] != str(Path(collection_reference).resolve())):
            raise ValueError("oracle collector reference invocation path changed; use a fresh directory")
        for line in path.read_text().splitlines():
            record = json.loads(line)
            key = reference_recipes.validate_record(record, manifest["reference_path"])
            if key in cache and cache[key] != record:
                raise ValueError("conflicting oracle records for source and recipe")
            cache[key] = record
    return cache_dir, path, cache


def oracle_record(cache, one):
    """Loaded caches use composite keys; plain maps support fixture observers.

    Persisted source-only maps cannot enter here: load_cache rejects their
    missing recipe manifest and invocation metadata before reading records.
    """
    return cache.get(reference_recipes.key(one), cache.get(one["source_sha256"]))


def record_oracle(reference, one):
    reference = Path(reference).resolve()
    recipe = reference_recipes.for_case(one)
    definition = reference_recipes.descriptor(recipe)
    result = {"source_sha256": one["source_sha256"], "recipe": recipe,
              "recipeSha256": reference_recipes.recipe_hash(recipe)}
    for profile in reference_recipes.PROFILES:
        arguments = reference_recipes.arguments(profile, recipe)
        raw = run(reference, arguments, one["source"], timeout=definition["timeoutSeconds"])
        for channel in ("stdout", "stderr"):
            original = (bytes.fromhex(raw[channel + "_bytes_hex"])
                        if channel + "_bytes_hex" in raw else raw[channel].encode())
            raw[channel + "_sha256"] = digest(original)
        raw.update(arguments=arguments, argv=[str(reference), *arguments],
                   environment=reference_recipes.environment(),
                   stdinSha256=one["source_sha256"],
                   timeoutSeconds=definition["timeoutSeconds"])
        result[profile] = raw
    return result


def collect_oracle(reference, registration, cases, evidence, workers):
    """Collect all five profiles once per distinct source/recipe pair."""
    identity = registration["identity"]
    cache_dir, cache_path, cache = load_cache(
        evidence, identity, digest(Path(reference).read_bytes()), collection_reference=reference)
    unique = sorted({reference_recipes.key(one): one for one in cases}.values(),
                    key=reference_recipes.key)
    missing = [one for one in unique if reference_recipes.key(one) not in cache
               or not all(profile in cache[reference_recipes.key(one)]
                          for profile in ("ascii", "utf8", "html", "tree", "lint"))]
    print(f"oracle cache: {len(cache)} cached, {len(missing)} to collect "
          f"(identity {identity})", flush=True)
    cache_dir.mkdir(parents=True, exist_ok=True)

    added = 0
    if missing:
        with cache_path.open("a", encoding="utf-8") as output:
            with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
                for i, entry in enumerate(pool.map(lambda one: record_oracle(reference, one), missing), 1):
                    output.write(json.dumps(entry, ensure_ascii=False) + "\n")
                    added += 1
                    if i % 500 == 0:
                        print(f"collected {i}/{len(missing)}", flush=True)
    manifest = {"identity": identity,
                "reference_sha256": digest(Path(reference).read_bytes()),
                "reference_path": str(Path(reference).resolve()),
                "policy_version": POLICY_VERSION,
                "records": len(set(cache) | {reference_recipes.key(one) for one in missing}),
                "expectations_from_product": False, **reference_recipes.cache_binding()}
    (cache_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"oracle collection complete: {added} new, {len(cache)} cache hits",
          flush=True)
    return cache_dir / "manifest.json"


def full_oracle_profiles(reference, source):
    """Retain the original default recipe for frozen selections."""
    return record_oracle(reference, {"source": source, "source_sha256": digest(source.encode())})


# ---------------------------------------------------------------------------
# Layer 3: admission (lint + tree qualification before any gold)
# ---------------------------------------------------------------------------

def admit(one, cache):
    record = oracle_record(cache, one)
    if record is None:
        return {"admission": "not-collected",
                "reason": "no cached oracle record; run --collect first"}
    lint = record["lint"]
    # Persisted records are authenticated with all five profiles. Old immutable
    # unit observers contain just their render/AST witnesses; qualify every
    # available profile without inventing missing receipt bytes. In production,
    # a partial HTML target observation cannot be admitted as a legal source.
    render_failed = (any(not reference_recipes.completed(record[p], 2)
                         for p in ("ascii", "utf8", "html", "tree") if p in record)
                     or not reference_recipes.completed(lint))
    admission, reason = None, None
    if render_failed:
        admission = "generator-defect"
        reason = "pristine render/AST profile failed; template must be fixed"
    elif lint["code"] == 0:
        admission = "legal"
    elif lint["code"] <= 2:
        admission = "native-diagnostics"
        reason = "warning-level diagnostics; structure remains comparable"
    else:
        admission = "recovery"
        reason = "error-level diagnostics; content compared as recovery only"
    # Tree-shape qualification for the multi-line column template: the three
    # declared columns must appear as three It body groups (BODY/Ta ownership).
    if admission != "generator-defect" and one["family"] == "column-positions" \
            and one["layout"] == "multiline":
        witness = tree_witness(record["tree"]["stdout"])
        if witness["it_body_groups"] < 3:
            admission = "generator-defect"
            reason = (f"multiline column template produced "
                      f"{witness['it_body_groups']} It body groups, expected 3")
    result = {"admission": admission, "reason": reason,
              "lint_code": lint["code"],
              "lint_classes": lint_classes(lint)}
    if one["family"] == "physical-row-handoffs":
        witness = tree_witness(record["tree"]["stdout"])
        lines = {line for line, _ in witness["text_positions"]}
        result["native_text_event"] = bool(
            lines & set(one.get("input_source_lines", [])))
    if one["family"] == "delimiter-arguments" \
            and one["delimiter"].startswith("literal") \
            and one["termination"] == "truncated-close":
        result["construction"] = "recovery-candidate: trailing incomplete backslash"
    if (one.get("dialect") == "mdoc" and one.get("family") == "output-owners"
            and one.get("inner") == "RS"):
        result["recovery_admission"] = result["admission"]
        result["admission"] = "generator-scope-gap"
        result["reason"] = "man RS/RE in mdoc does not witness the declared RS owner"
    if one["family"] == "corrected-owner-scopes":
        witness = tree_witness(record["tree"]["stdout"])
        if not witness["kinds"].get("RS") or "RS (body)" not in record["tree"]["stdout"]:
            result["admission"] = "generator-defect"
            result["reason"] = "corrected man source did not create the required RS BODY"
    return result


# ---------------------------------------------------------------------------
# Layer 2: product replay in one process, aggregated failures
# ---------------------------------------------------------------------------

PRODUCT_TEXT = ["--input", "-", "--input-format", "roff", "--display", "direct",
                "--color", "never", "--format", "text"]
PRODUCT_ANSI = ["--input", "-", "--input-format", "roff", "--display", "direct",
                "--color", "always", "--format", "text"]
PRODUCT_JSON = ["--input", "-", "--input-format", "roff", "--format", "json"]
PRODUCT_MARKDOWN = ["--input", "-", "--input-format", "roff", "--display",
                    "direct", "--format", "markdown"]
IDENTITY_MACROS = ("Lk", "UR", ".Mt", ".MR", ".Sx", ".In", ".Bx", ".Xr")


def product_binding(product, product_head=None):
    return {
        "binary": str(Path(product).resolve()),
        "sha256": digest(Path(product).read_bytes()),
        "head": product_head or subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True,
            text=True, check=False).stdout.strip(),
        "dirty_tree": bool(subprocess.run(
            ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True,
            text=True, check=False).stdout.strip()),
    }


def replay_product(product, cases, evidence, workers, product_head=None):
    """Run the product once per unique source; aggregate per-case results."""
    binding = product_binding(product, product_head)
    product_sha256 = binding["sha256"]
    by_hash = {}
    for one in cases:
        by_hash.setdefault(one["source_sha256"], one)
    terminal = "NEXT"

    def execute(one):
        source = one["source"]
        plain = run(product, PRODUCT_TEXT, source)
        ansi = run(product, PRODUCT_ANSI, source)
        result = {
            "plain_code": plain["code"],
            "product_error": (plain["code"] != 0 or ansi["code"] != 0
                              or not plain["utf8_valid"] or not ansi["utf8_valid"]),
            "ansi_parity": unstyle(plain["stdout"]) == unstyle(ansi["stdout"]),
            "stdout_sha256": {name: digest(run_output["stdout"].encode())
                              for name, run_output in
                              (("plain", plain), ("ansi", ansi))},
        }
        result["product_rows"] = body_rows(
            plain["stdout"], terminal, eof_tail=one.get("tail") == "eof")
        encoded = run(product, PRODUCT_JSON, source)
        result["json_code"] = encoded["code"]
        result["marker_leak"] = ("\\u0000mant:" in encoded["stdout"]
                                 if encoded["code"] == 0 else None)
        if encoded["code"] != 0 or not encoded["utf8_valid"]:
            result["product_error"] = True
        else:
            try:
                bundle = json.loads(encoded["stdout"])
                result["product_region"] = acceptance_regions.select_product_region(
                    plain["stdout"], bundle)
                result["external_targets"] = acceptance_comparison.product_external_targets(bundle)
            except (json.JSONDecodeError, TypeError, AttributeError) as error:
                result["product_error"] = True
                result["json_error"] = str(error)
        raw = {"text": plain, "ansi": ansi, "json": encoded}
        if any(macro in source for macro in IDENTITY_MACROS):
            markdown = run(product, PRODUCT_MARKDOWN, source)
            raw["markdown"] = markdown
            result["markdown_code"] = markdown["code"]
            if markdown["code"] != 0 or not markdown["utf8_valid"]:
                result["product_error"] = True
        result["stdout_sha256"]["json"] = digest(encoded["stdout"].encode())
        return result, raw

    results = {}
    failures = 0
    unique = sorted(by_hash.values(), key=lambda one: one["source_sha256"])
    with (evidence / "product-raw.jsonl").open("w", encoding="utf-8") as raw_output:
        with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
            for i, (one, (result, raw)) in enumerate(
                    zip(unique, pool.map(execute, unique)), 1):
                results[one["source_sha256"]] = result
                raw_output.write(json.dumps({"source_sha256": one["source_sha256"],
                                             "profiles": raw}, ensure_ascii=False) + "\n")
                if result["product_error"]:
                    failures += 1
                if i % 500 == 0:
                    print(f"replayed {i}/{len(unique)}", flush=True)
    (evidence / "replay-results.json").write_text(
        json.dumps({sha: result for sha, result in results.items()},
                   indent=1, sort_keys=True) + "\n")
    if digest(Path(product).read_bytes()) != product_sha256:
        raise SystemExit("product binary changed during replay; rerun after the build finishes")
    (evidence / "replay-binding.json").write_text(json.dumps({
        "binary": str(Path(product).resolve()), "sha256": product_sha256,
        "unique_sources": len(results), "results_sha256": digest(
            (evidence / "replay-results.json").read_bytes()),
        "raw_sha256": digest((evidence / "product-raw.jsonl").read_bytes()),
        "product_binding": binding,
    }, indent=2) + "\n")
    print(f"product replay complete: {len(unique)} unique sources, "
          f"{failures} with product errors", flush=True)
    return results


# ---------------------------------------------------------------------------
# Ledger assembly
# ---------------------------------------------------------------------------

def uncovered_axes(one, report):
    """Declared axes still awaiting comparator wiring beyond screening."""
    policy = one["policy"]
    axes = [axis for axis, outcome in report.items() if outcome == "uncovered"]
    if policy["identity"] == "rich-inline":
        axes.append("identity:rich-inline")
    # The full CLI layer covers display/edge rows and external URI occurrence.
    # Source/style/scalar ranges and real interactive consumers are covered
    # by the Rust adapter's cards, not inferred from a successful CLI render.
    axes.extend(("style", "source", "scalar-range", "tui", "query"))
    return axes


def build_ledger(cases, cache, product_results, product_binding, frozen,
                 oracle_binding=None):
    families = {}
    case_rows = {}
    terminal = "NEXT"
    policies = acceptance_comparison.load_policies()
    oracle_binding = oracle_binding or {}
    for one in cases:
        record = oracle_record(cache, one) or {}
        admission_info = admit(one, cache)
        eof = one.get("tail") == "eof"
        native_rows = body_rows(record.get("utf8", {}).get("stdout", ""),
                                terminal, eof_tail=eof)
        product_result = product_results.get(one["source_sha256"], {})
        product_rows = product_result.get("product_rows")
        screen = screening(native_rows, product_rows)
        policy, policy_error = acceptance_comparison.qualified_policy(
            one, record, oracle_binding, policies)
        native_region = acceptance_regions.select_native_region(
            record.get("utf8", {}).get("stdout", ""),
            record.get("tree", {}).get("stdout", ""))
        expected_region = policy.get("expected_region", native_region)
        report = acceptance_comparison.compare_axes(
            expected_region, product_result.get("product_region", {}), policy)
        if policy_error:
            # A changed source or oracle cannot reuse a bound expectation.
            # This is failed evidence authentication, not an unimplemented
            # consumer axis that can merely remain review/uncovered.
            report["policy-binding"] = False
        if "external_targets" in product_result and "html" in record:
            expected_targets = policy.get("external_targets",
                acceptance_comparison.native_external_targets(record["html"]["stdout"]))
            report["identity:external-occurrences"] = (
                expected_targets == product_result["external_targets"])
        else:
            report["identity:external-occurrences"] = "uncovered"
        observations = report
        report = acceptance_comparison.qualified_axes(admission_info["admission"], report)
        missing_axes = uncovered_axes(one, report)
        if policy_error:
            missing_axes.append("policy-binding")
        status, failing = acceptance_comparison.verdict(
            admission_info["admission"], report, product_result, missing_axes)
        row = {
            "id": one["id"], "cohort": one["cohort"], "family": one["family"],
            "axes": {k: v for k, v in one.items()
                     if k not in ("source", "source_sha256", "policy")},
            "source_sha256": one["source_sha256"],
            "policy": one["policy"],
            **admission_info,
            "screening": screen,
            "axis_report": report,
            "region": native_region,
            "presentation_policy": policy,
            "policy_binding_error": policy_error,
            "product": {k: v for k, v in product_result.items()
                        if k not in ("native_rows", "product_rows")},
            "status": status,
            "failing_axes": failing,
            "uncovered_axes": missing_axes,
            "consumer_covered": ["cli-text-screening", "cli-ansi-screening"],
        }
        if product_result.get("marker_leak") is not None:
            row["consumer_covered"].append("cli-json-screening")
        if "markdown_code" in product_result:
            row["consumer_covered"].append("cli-markdown-screening")
        if admission_info["admission"] in acceptance_comparison.UNQUALIFIED_ADMISSIONS:
            row["unqualified_axis_observations"] = observations
        case_rows[one["id"]] = row

    for one in cases:
        row = case_rows[one["id"]]
        summary = families.setdefault(one["family"], {
            "cohort": one["cohort"],
            "generated": 0,
            "source_unique": set(),
            "admission": collections.Counter(),
            "screening": collections.Counter(),
            "axis_asserted": collections.defaultdict(collections.Counter),
            "consumer_covered": collections.defaultdict(int),
            "pass": 0, "fail": 0, "review": 0, "uncovered": 0,
            "failing": collections.Counter(), "na_reasons": collections.Counter(),
        })
        summary["generated"] += 1
        summary["source_unique"].add(one["source_sha256"])
        summary["admission"][row["admission"]] += 1
        for axis, outcome in row["axis_report"].items():
            summary["axis_asserted"][axis][str(outcome).lower()] += 1
        for consumer in row["consumer_covered"]:
            summary["consumer_covered"][consumer] += 1
        summary[row["status"]] += 1
        if row["uncovered_axes"]:
            summary["uncovered"] += 1
        for axis in row["failing_axes"]:
            summary["failing"][axis] += 1
        if row["screening"].get("extraction") != "ok":
            summary["screening"]["extraction-not-applicable"] += 1
        else:
            for axis in ("glyph_equal", "row_equal", "indent_equal"):
                summary["screening"][f"{axis}={row['screening'][axis]}"] += 1

    for summary in families.values():
        summary["source_unique"] = len(summary["source_unique"])
        summary["axis_asserted"] = {axis: dict(counts) for axis, counts
                                    in summary["axis_asserted"].items()}
        summary["consumer_covered"] = dict(summary["consumer_covered"])
        summary["admission"] = dict(summary["admission"])
        summary["screening"] = dict(summary["screening"])
        summary["failing"] = dict(summary["failing"])
        summary["na_reasons"] = dict(summary["na_reasons"])

    coverage = []
    if frozen:
        for transition in roff_acceptance_cases.COVERAGE_TRANSITIONS:
            entries = []
            for name in transition["selections"]:
                selection = frozen[name]
                row = case_rows[selection["case"]["id"]]
                entries.append({"selection": name, "case_id": row["id"],
                                "role": selection["role"],
                                "admission": row["admission"],
                                "status": row["status"]})
            coverage.append({"transition": transition["transition"],
                             "axes": transition["axes"], "cases": entries,
                             "consumers": ["cli-text-screening",
                                           "cli-ansi-screening",
                                           "cli-json-screening",
                                           "cli-markdown-screening",
                                           "oracle-five-profiles"]})
    return {
        "schema": LEDGER_SCHEMA,
        "policy_version": POLICY_VERSION,
        "product_binding": product_binding,
        "oracle_binding": oracle_binding,
        "case_count": len(case_rows),
        "families": dict(sorted(families.items())),
        "coverage": coverage,
        "empty_text_witness": roff_acceptance_cases.EMPTY_TEXT_WITNESS,
        "notes": [
            "screening equalities are diagnostics only; verdict comes from independent axes.",
            "Native diagnostics qualify inputs and do not override applicable assertion failures.",
            "Content edges are retained; source-bound presentation policies change only named axes.",
            "Uncovered consumer/style/source/range dimensions keep otherwise matching cases review.",
        ],
        "cases": case_rows,
    }




# ---------------------------------------------------------------------------
# Freeze: checked-in representatives with full oracle evidence
# ---------------------------------------------------------------------------

def html_targets(stdout):
    """Authored Lk href targets from pristine HTML via the shared parser."""
    from scripts.roff.fixtures.generate_roff_execution_fixtures import Links

    parser = Links()
    parser.feed(stdout)
    return parser.targets



def freeze(frozen, product_binding, reference, ledger_path):
    cases_dir = ACCEPTANCE / "cases"
    oracle_dir = ACCEPTANCE / "oracle"
    cases_dir.mkdir(parents=True, exist_ok=True)
    if not ledger_path.exists():
        raise SystemExit("freeze requires the full ledger; run with --ledger")
    full_ledger = json.loads(ledger_path.read_text())
    frozen_rows = {}
    for name, selection in sorted(frozen.items()):
        one = selection["case"]
        profiles = full_oracle_profiles(reference, one["source"])
        source_path = cases_dir / f"{one['id']}.roff"
        source_path.write_bytes(one["source"].encode())
        record = {
            "selection": name,
            "role": selection["role"],
            "note": selection["note"],
            "id": one["id"],
            "cohort": one["cohort"],
            "family": one["family"],
            "axes": {k: v for k, v in one.items()
                     if k not in ("source", "source_sha256", "policy")},
            "source_sha256": one["source_sha256"],
            "policy": one["policy"],
            "oracle": {
                profile: {"code": value["code"],
                          "stdout_sha256": value["stdout_sha256"],
                          "stderr_sha256": value.get("stderr_sha256"),
                          "stdout": value["stdout"],
                          "stderr": value["stderr"]}
                for profile, value in profiles.items()},
            "utf8_rows": body_rows(profiles["utf8"]["stdout"], "NEXT"),
            "ascii_rows": body_rows(profiles["ascii"]["stdout"], "NEXT"),
            "targets": html_targets(profiles["html"]["stdout"]),
            "tree_witness": tree_witness(profiles["tree"]["stdout"]),
            "admission": {k: full_ledger["cases"][one["id"]][k]
                          for k in ("admission", "reason", "lint_code",
                                    "lint_classes")},
            "replay_status": full_ledger["cases"][one["id"]]["status"],
            "replay_failing_axes": full_ledger["cases"][one["id"]]["failing_axes"],
            "product_binding": product_binding,
            "expectations_from_product": False,
        }
        (oracle_dir / f"{one['id']}.json").write_text(
            json.dumps(record, ensure_ascii=False, indent=1, sort_keys=True) + "\n")
        frozen_rows[name] = {"id": one["id"], "family": one["family"],
                             "role": selection["role"], "note": selection["note"]}
    summary = dict(full_ledger)
    summary["cases"] = {one["id"]: full_ledger["cases"][one["id"]]
                        for one in (s["case"] for s in frozen.values())}
    summary["frozen"] = frozen_rows
    (ACCEPTANCE / "ledger.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=1, sort_keys=True) + "\n")
    print(f"frozen {len(frozen_rows)} selections into {ACCEPTANCE}")


def check_frozen(reference):
    """Verify checked-in frozen records against the current pristine oracle."""
    failures = []
    for path in sorted((ACCEPTANCE / "oracle").glob("*.json")):
        record = json.loads(path.read_text())
        source = (ACCEPTANCE / "cases" / f"{path.stem}.roff").read_bytes()
        if digest(source) != record["source_sha256"]:
            failures.append(f"{path.stem}: source bytes differ from recorded hash")
            continue
        for profile in ("ascii", "utf8", "html", "tree", "lint"):
            arguments = reference_recipes.arguments(profile)
            completed = roff_fixture_reference.run_reference(
                reference, arguments, input_bytes=source, timeout=30,
                env=reference_recipes.environment(),
                check=False)
            if digest(completed.stdout) != record["oracle"][profile]["stdout_sha256"]:
                failures.append(f"{path.stem}/{profile}: oracle output differs")
    if failures:
        raise SystemExit("frozen record check failed:\n" + "\n".join(failures))
    print(f"frozen records verified against oracle: "
          f"{len(list((ACCEPTANCE / 'oracle').glob('*.json')))} cases")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def generation_plan(cases):
    """Canonical deterministic dump for idempotence verification."""
    return json.dumps([{"id": one["id"], "family": one["family"],
                        "cohort": one["cohort"],
                        "source_sha256": one["source_sha256"],
                        "policy": one["policy"]} for one in cases],
                      ensure_ascii=False, indent=1, sort_keys=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path,
                        default=ROOT / "target/roff-acceptance-replay")
    parser.add_argument("--reference", type=Path,
                        default=Path(os.environ.get("MANT_REFERENCE", REFERENCE)))
    parser.add_argument("--product", type=Path,
                        default=Path(os.environ.get("MANT_PRODUCT", "")) if
                        os.environ.get("MANT_PRODUCT") else None)
    parser.add_argument("--product-head", default=None,
                        help="commit the product binary was built from when it "
                             "differs from this checkout's HEAD")
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--families", help="comma-separated family filter prefix")
    parser.add_argument("--check-generation", action="store_true",
                        help="print canonical generation plan hash and exit")
    parser.add_argument("--collect", action="store_true",
                        help="ensure the oracle cache for all sources")
    parser.add_argument("--replay", action="store_true",
                        help="run the product screening layer")
    parser.add_argument("--ledger", action="store_true",
                        help="write the full admission ledger")
    parser.add_argument("--freeze", action="store_true",
                        help="write the checked-in frozen selection")
    parser.add_argument("--check-frozen", action="store_true",
                        help="verify checked-in frozen oracle records")
    args = parser.parse_args()

    cases = all_cases()
    if args.families:
        wanted = tuple(args.families.split(","))
        cases = [one for one in cases if one["family"].startswith(wanted)]
    frozen = select_frozen(cases) if not args.families else None

    if args.check_generation:
        plan = generation_plan(cases)
        print(f"cases: {len(cases)}  plan_sha256: {digest(plan.encode())}")
        families = collections.Counter(one["family"] for one in cases)
        print(json.dumps(dict(sorted(families.items())), indent=1))
        return

    evidence = args.evidence.resolve()
    if not evidence.is_relative_to(ROOT / "target"):
        raise SystemExit("evidence must stay below the repository target directory")
    evidence.mkdir(parents=True, exist_ok=True)
    reference = args.reference if args.reference.is_absolute() \
        else ROOT / args.reference
    if args.check_frozen:
        roff_fixture_reference.verified_reference(ROOT, reference)
        check_frozen(reference)
        return

    modes = [args.collect, args.replay, args.ledger, args.freeze]
    if not any(modes):
        parser.error("choose at least one of --collect/--replay/--ledger/--freeze "
                     "(or --check-generation/--check-frozen)")
    product = args.product
    if (args.replay or args.freeze) and product is None:
        parser.error("--product or MANT_PRODUCT is required for replay/freeze")

    registration = roff_fixture_reference.verified_reference(ROOT, reference)
    print(f"oracle preflight passed: {registration['identity']}", file=sys.stderr)

    cache_dir, cache_path, cache = load_cache(evidence, registration["identity"], digest(reference.read_bytes()))
    if args.collect:
        collect_oracle(reference, registration, cases, evidence, args.workers)
        cache_dir, cache_path, cache = load_cache(evidence, registration["identity"], digest(reference.read_bytes()))
    missing = [one["id"] for one in cases if reference_recipes.key(one) not in cache]
    if args.replay and missing:
        raise SystemExit(f"{len(missing)} sources lack oracle records; "
                         f"run --collect first (example {missing[0]})")

    product_results = {}
    if args.replay:
        product_results = replay_product(product, cases, evidence, args.workers,
                                         args.product_head)
    elif (args.ledger or args.freeze) and (evidence / "replay-results.json").exists():
        binding_path = evidence / "replay-binding.json"
        if not binding_path.exists():
            raise SystemExit("persisted replay has no product binding; rerun --replay")
        binding = json.loads(binding_path.read_text())
        if digest((evidence / "replay-results.json").read_bytes()) != binding["results_sha256"]:
            raise SystemExit("persisted replay bytes differ from their binding; rerun --replay")
        raw_path = evidence / "product-raw.jsonl"
        if not raw_path.exists() or digest(raw_path.read_bytes()) != binding["raw_sha256"]:
            raise SystemExit("persisted raw replay differs from its binding; rerun --replay")
        if product is not None and digest(Path(product).read_bytes()) != binding["sha256"]:
            raise SystemExit("persisted replay belongs to a different product binary; rerun --replay")
        product_results = json.loads(
            (evidence / "replay-results.json").read_text())
        print(f"loaded persisted product replay: {len(product_results)} sources")

    ledger_path = evidence / "ledger.json"
    if args.ledger or args.freeze:
        binding_path = evidence / "replay-binding.json"
        if not binding_path.exists():
            raise SystemExit("ledger requires a bound product replay; run --replay")
        replay_binding = json.loads(binding_path.read_text())
        candidate_binding = replay_binding["product_binding"]
        ledger = build_ledger(cases, cache, product_results, candidate_binding,
                              frozen, {"identity": registration["identity"],
                                       "reference_sha256": digest(reference.read_bytes())})
        ledger_path.write_text(
            json.dumps(ledger, ensure_ascii=False, indent=1, sort_keys=True) + "\n")
        counts = collections.Counter(row["status"] for row in ledger["cases"].values())
        print(f"ledger written: {ledger_path} ({dict(counts)})")
    if args.freeze:
        freeze(frozen, candidate_binding, reference, ledger_path)


if __name__ == "__main__":
    main()
