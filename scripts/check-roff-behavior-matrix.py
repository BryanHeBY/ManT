#!/usr/bin/env python3
"""Replay source-bound layout/content probes against an explicit native oracle.

Developer audit only: does not build binaries, rewrite acceptance records, or
turn historic reviews into waivers. All generated sources are Apache-2.0.
"""

from __future__ import annotations

import argparse
import copy
from collections import Counter
from datetime import datetime, timezone
import hashlib
import importlib
import json
from pathlib import Path
import re
import subprocess

from roff_rendering_frame import prepare_frame
from roff_reference import reference_environment, run_renderer


ROOT = Path(__file__).resolve().parents[1]
RECORDS = (
    ROOT / "tests/fixtures/roff/LAYOUT_ACCEPTANCE.json",
    ROOT / "tests/fixtures/roff/LAYOUT_FOLLOWUP_ACCEPTANCE.json",
)
MAN = '.TH MATRIX 1 "2026-09-11"\n.SH TEST\n'
MDOC = '.Dd September 11, 2026\n.Dt MATRIX 1\n.Os\n.Sh TEST\n'


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_evidence(path: Path) -> dict:
    return {"path": str(path), "sha256": digest(path.read_bytes())}


def save_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def cases() -> list[dict]:
    result = []
    for record in RECORDS:
        for original in json.loads(record.read_text())["probes"]["cases"]:
            item = dict(original)
            item["id"] = record.stem + "/" + item["name"]
            item["origin"] = str(record.relative_to(ROOT))
            actual = digest(item["source"].encode())
            if item.get("sourceSha256", actual) != actual:
                raise ValueError(f"source SHA mismatch: {item['id']}")
            item["sourceSha256"] = actual
            result.append(item)

    def add(name: str, source: str, required=(), forbidden=(), dimensions=()):
        result.append({
            "id": "generated/" + name, "name": name, "source": source,
            "origin": "original Apache-2.0 generated regression source",
            "sourceSha256": digest(source.encode()),
            "requiredVisible": list(required), "forbiddenVisible": list(forbidden),
            "dimensions": list(dimensions),
        })

    # Small visible operands intentionally defeat length-based token exclusions.
    for dialect, header in (("man", MAN), ("mdoc", MDOC)):
        for literal in (False, True):
            for name, request in (
                ("br", ".br"), ("sp0", ".sp 0"), ("sp2", ".sp 2"),
                ("paragraph", ".PP" if dialect == "man" else ".Pp"),
                ("fi", ".fi"), ("nf", ".nf"), ("ce", ".ce 1"),
            ):
                source = header + (".nf\n" if literal else "")
                source += "MATRIXALPHA x 中 7\n" + request + "\n"
                source += "MATRIXBETA y 文 8\n.fi\nMATRIXOMEGA\n"
                add(f"{dialect}-{'literal' if literal else 'filled'}-{name}", source,
                    ("MATRIXALPHA", "MATRIXBETA", "MATRIXOMEGA", "中", "文"),
                    dimensions=("flow", "short-content", "control-operands"))

    for group in ("ce", "rj"):
        for count in ("", "badcount", "2"):
            source = MAN + f"MATRIXBEFORE\n.{group}" + (" " + count if count else "")
            source += "\nMATRIXALPHA 7\nMATRIXBETA 8\n.br\nMATRIXAFTER\n"
            add(f"{group}-count-{count or 'default'}", source,
                ("MATRIXBEFORE", "MATRIXALPHA", "MATRIXBETA", "MATRIXAFTER"),
                ("badcount",), ("captured-group", "control-operands"))
        for request in ("br", "fi", "nf", "ti"):
            source = MAN + f"MATRIXBEFORE\n.{group} 2\nMATRIXALPHA 7\n"
            source += f".{request}" + (" 13n" if request == "ti" else "")
            source += "\nMATRIXBETA 8\n.fi\nMATRIXAFTER\n"
            add(f"{group}-nested-{request}", source,
                ("MATRIXBEFORE", "MATRIXALPHA", "MATRIXBETA", "MATRIXAFTER"),
                ("13n",), ("captured-group", "control-operands", "hard-rows"))

    for transition in (".br", ".nf", ".fi", ".EX", ".sp 0", ".sp 2"):
        source = MAN + "MATRIXBASE\n.RS 3\n.HP 7\nMATRIXFIRST one two three four five six\n"
        source += transition + "\nMATRIXSECOND seven eight nine ten\n"
        source += (".EE\n" if transition == ".EX" else ".fi\n")
        source += ".RE\n.PP\nMATRIXOUTER\n"
        add("hp-rs-" + transition[1:].replace(" ", "-"), source,
            ("MATRIXBASE", "MATRIXFIRST", "MATRIXSECOND", "MATRIXOUTER"),
            dimensions=("hanging", "relative-indent", "mode-boundary", "narrow-width"))

    for wrapper, opening, closing in (
        ("paragraph", ".Pp\n", ""),
        ("display", ".Bd -literal -offset 3n\n", ".Ed\n"),
        ("item", ".Bl -item\n.It\n", ".El\n"),
    ):
        source = MDOC + "MATRIXBEFORE\n" + opening
        source += ".Tg Matrix.Target\nMATRIXTARGET\n" + closing + ".Pp\nMATRIXAFTER\n"
        add("target-" + wrapper, source,
            ("MATRIXBEFORE", "MATRIXTARGET", "MATRIXAFTER"),
            ("Matrix.Target",), ("target-location", "zero-width", "container-boundary"))
        result[-1]["expectedTarget"] = {"id": "matrix-target", "text": "MATRIXTARGET"}
    return result


def invoke(command: list[str], directory: Path, stem: str, timeout: float) -> dict:
    env = reference_environment()
    env["NO_COLOR"] = "1"
    code, stdout, stderr = run_renderer(command, timeout, env, binary_output=True)
    stderr = stderr.encode()
    status = "timeout" if code == 124 else "execution-error" if code == 125 else "completed"
    out = directory / (stem + ".stdout")
    err = directory / (stem + ".stderr")
    out.write_bytes(stdout)
    err.write_bytes(stderr)
    return {"command": command, "status": status, "returncode": code,
            "stdout": file_evidence(out), "stderr": file_evidence(err)}


def output(invocation: dict) -> str:
    return Path(invocation["stdout"]["path"]).read_text(errors="replace")


def body_projection(raw: str, source: str, reference: bool) -> tuple[str, dict]:
    return prepare_frame(raw, source, reference=reference)


def compare(function, reference: str, mant: str, source: str) -> dict:
    if function is None:
        return {"status": "uncovered", "coverage": {}, "findings": [],
                "reason": "Comparator module unavailable; no equality claim."}
    try:
        value = function(reference, mant, source)
        if not isinstance(value, dict) or "status" not in value:
            raise ValueError("comparator must return object containing status")
        return value
    except Exception as exc:
        return {"status": "error", "coverage": {}, "findings": [],
                "reason": f"{type(exc).__name__}: {exc}"}


def load_comparator(module: str, function: str):
    try:
        loaded = importlib.import_module(module)
    except ModuleNotFoundError as exc:
        if exc.name != module:
            raise
        return None, None
    return getattr(loaded, function), file_evidence(Path(loaded.__file__))


def visible_assertions(case: dict, rendered: str) -> dict:
    findings = []
    for token in case.get("requiredVisible", []):
        if rendered.count(token) != 1:
            findings.append({"kind": "visible-count", "token": token,
                             "expected": 1, "actual": rendered.count(token)})
    for token in case.get("forbiddenVisible", []):
        if token in rendered:
            findings.append({"kind": "operand-or-target-leak", "token": token})
    return {"status": "review" if findings else "covered", "findings": findings,
            "coverage": {"assertions": len(case.get("requiredVisible", [])) +
                         len(case.get("forbiddenVisible", []))}}


def probe_assertions(probe: dict, case: dict, widths: list[int]) -> dict:
    findings, uncovered = [], []
    if probe.get("schema") != "mant-dev-geometry-audit-v1":
        return {"status": "uncovered", "findings": [], "coverage": {"reasons": ["unsupported-probe-schema"]}}
    if not probe.get("complete") or not probe.get("body", {}).get("complete"):
        uncovered.append("probe-or-body-incomplete")
    renders = probe.get("renders", [])
    if sorted(r.get("width") for r in renders) != sorted(widths):
        uncovered.append("missing-or-duplicate-width")
    checked_targets = 0
    for render in renders:
        width = render["width"]
        if not render.get("rows_complete") or not render.get("cells_complete"):
            uncovered.append(f"width-{width}-rows-or-cells-incomplete")
        rows = {r["row"]: r for r in render.get("rows", [])}
        for row in rows.values():
            if row.get("over_width"):
                findings.append({"kind": "viewport-row-overflow", "width": width,
                                 "row": row["row"], "columns": row.get("columns"), "text": row["text"]})
            if "spans" in row and "".join(s["text"] for s in row["spans"]) != row["text"]:
                findings.append({"kind": "row-span-text-mismatch", "width": width, "row": row["row"]})
        for anchor in render.get("anchors", []):
            if anchor.get("row") not in rows or not anchor.get("in_row_range"):
                findings.append({"kind": "target-outside-captured-rows", "width": width, "anchor": anchor})
        target = case.get("expectedTarget")
        if target:
            matches = [a for a in render.get("anchors", []) if a["id"] == target["id"]]
            checked_targets += 1
            if len(matches) != 1:
                findings.append({"kind": "target-cardinality", "width": width, "target": target, "actual": len(matches)})
            else:
                actual_row = rows.get(matches[0].get("row"), {})
                # Inspect the independently captured row, not the anchor's
                # descriptive line_text (which can itself be stale/wrong).
                if target["text"] not in actual_row.get("text", ""):
                    findings.append({"kind": "target-wrong-source-row", "width": width,
                                     "target": target, "anchor": matches[0], "row": actual_row.get("text")})
    return {"status": "review" if findings else "partial" if uncovered else "covered",
            "findings": findings, "coverage": {"widths": len(renders), "targetSourceRowChecks": checked_targets,
                                               "uncovered": uncovered, "targetColumns": "not exposed by public API"}}


def sensitivity_checks(content, geometry) -> list[dict]:
    # Baseline must first be covered: a pre-existing finding cannot count as a
    # detected injected fault. These clean, furniture-free samples are not
    # substitutes for comparing actual process output above.
    source = MAN + ".nf\nALPHA x 中 7\nBETA repeat repeat\n.sp 2\nGAMMA\n.fi\n"
    base = "TEST\nALPHA x 中 7\nBETA repeat repeat\n\n\nGAMMA\n"
    mutations = (
        ("short-token-loss", content, base.replace(" x ", " ")),
        ("unicode-loss", content, base.replace("中", "")),
        ("repeated-token-loss", content, base.replace("repeat repeat", "repeat")),
        ("operand-leak", content, base.replace("BETA", "13n BETA")),
        ("local-order", content, base.replace("ALPHA x 中 7\nBETA repeat repeat", "BETA repeat repeat\nALPHA x 中 7")),
        ("explicit-row-merge", geometry, base.replace("7\nBETA", "7 BETA")),
        ("blank-row-loss", geometry, base.replace("\n\n\n", "\n\n")),
        ("relative-origin", geometry, base.replace("BETA", "    BETA")),
    )
    result = []
    for name, function, mutant in mutations:
        mutation_source = source + ".ti 13n\n" if name == "operand-leak" else source
        baseline = compare(function, base, base, mutation_source)
        changed = compare(function, base, mutant, mutation_source)
        ready = baseline["status"] == "covered"
        detected = ready and changed["status"] == "review" and bool(changed.get("findings"))
        result.append({"id": name, "status": "detected" if detected else "missed" if ready else "uncovered",
                       "source": mutation_source, "reference": base, "mutant": mutant,
                       "baseline": baseline, "comparison": changed})
    sample = {"schema": "mant-dev-geometry-audit-v1", "complete": True, "body": {"complete": True},
              "renders": [{"width": 20, "rows_complete": True, "cells_complete": True,
                           "rows": [{"row": 0, "text": "MATRIXBEFORE"}, {"row": 1, "text": "MATRIXTARGET"}],
                           "anchors": [{"id": "matrix-target", "row": 1, "in_row_range": True,
                                        "line_text": "MATRIXTARGET"}]}]}
    case = {"expectedTarget": {"id": "matrix-target", "text": "MATRIXTARGET"}}
    baseline = probe_assertions(sample, case, [20])
    mutant = copy.deepcopy(sample)
    mutant["renders"][0]["anchors"][0]["row"] = 0
    changed = probe_assertions(mutant, case, [20])
    result.append({"id": "target-relocation", "status": "detected" if baseline["status"] == "covered"
                   and changed["status"] == "review" else "missed", "baseline": baseline,
                   "comparison": changed, "mutation": "Move only target row to adjacent source owner, preserving ID set and line_text."})
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mant", type=Path, default=ROOT / "target/debug/mant")
    parser.add_argument("--reference", type=Path,
                        help="Explicit unpatched pinned CVS mandoc binary; no PATH fallback")
    parser.add_argument("--geometry-probe", type=Path)
    parser.add_argument("--output", type=Path, help="New local evidence directory")
    parser.add_argument("--self-test", action="store_true", help="Run input, framing and comparator sensitivity tests without binaries")
    parser.add_argument("--widths", default="20,40,80,120")
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--case", action="append", default=[], help="Case ID substring; repeatable")
    parser.add_argument("--verify", action="store_true", help="Fail on any review, error, partial or uncovered dimension")
    args = parser.parse_args()
    if args.self_test:
        from roff_rendering_frame import self_test
        self_test()
        all_cases = cases()
        assert len(all_cases) == 112 and len({c["id"] for c in all_cases}) == 112
        assert sum(c["origin"].startswith("tests/") for c in all_cases) == 61
        content, _ = load_comparator("roff_content_compare", "compare_content")
        geometry, _ = load_comparator("roff_layout_geometry", "compare_layout_geometry")
        outcomes = sensitivity_checks(content, geometry)
        assert all(x["status"] == "detected" for x in outcomes), outcomes
        print("behavior matrix self-tests passed: 112 sources, 9 detected fault classes")
        return 0
    if args.reference is None or args.output is None:
        parser.error("--reference and --output are required for a replay")
    widths = [int(x) for x in args.widths.split(",")]
    if not widths or any(x < 10 or x > 500 for x in widths) or args.timeout <= 0:
        parser.error("widths must be 10..500 and timeout must be positive")
    selected = [c for c in cases() if not args.case or any(x in c["id"] for x in args.case)]
    if not selected:
        parser.error("no matching cases")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    binaries = {"mant": args.mant.resolve(), "reference": args.reference.resolve()}
    if args.geometry_probe:
        binaries["geometry"] = args.geometry_probe.resolve()
    before = {k: file_evidence(v) for k, v in binaries.items()}
    content, content_identity = load_comparator("roff_content_compare", "compare_content")
    geometry, geometry_identity = load_comparator("roff_layout_geometry", "compare_layout_geometry")
    provenance = {
        "schema": "mant-roff-behavior-matrix/1", "startedAt": datetime.now(timezone.utc).isoformat(),
        "producerCommit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "gitStatus": subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True),
        "binariesBefore": before, "runner": file_evidence(Path(__file__)),
        "comparators": {"content": content_identity, "geometry": geometry_identity},
        "framing": file_evidence(ROOT / "scripts/roff_rendering_frame.py"),
        "subprocessBoundary": file_evidence(ROOT / "scripts/roff_reference.py"),
        "records": [file_evidence(p) for p in RECORDS], "widths": widths,
        "locale": "C.UTF-8", "timeoutSeconds": args.timeout,
        "referenceTrust": "Caller supplies unpatched pinned CVS binary; binary hash is evidence, not proof of source-to-build mapping.",
    }
    save_json(args.output / "provenance.json", provenance)
    statuses = Counter()
    rows = []
    for index, case in enumerate(selected):
        directory = args.output / f"{index:03d}-{re.sub('[^A-Za-z0-9_.-]', '_', case['id'])}"
        directory.mkdir()
        source = directory / "source.roff"
        source.write_bytes(case["source"].encode())
        row = {"case": case, "source": file_evidence(source), "comparisons": []}
        cli = invoke([str(binaries["mant"]), "--input", str(source), "--input-format", "roff",
                      "--format", "text", "--display", "direct", "--color", "never"], directory, "mant", args.timeout)
        row["mant"] = cli
        cli_body, cli_cleanup = body_projection(output(cli), case["source"], False)
        (directory / "mant.body").write_text(cli_body)
        row["mantBodyProjection"] = cli_cleanup
        statuses["frame:" + cli_cleanup["status"]] += 1
        if cli["returncode"] != 0:
            statuses["process-error"] += 1
        if case.get("requiredVisible"):
            row["visibleAssertions"] = visible_assertions(case, output(cli))
            statuses["assertions:" + row["visibleAssertions"]["status"]] += 1
        for width in widths:
            ref = invoke([str(binaries["reference"]), "-Tutf8", "-O", f"width={width}", str(source)],
                         directory, f"reference-{width}", args.timeout)
            if ref["returncode"] != 0:
                statuses["process-error"] += 1
            ref_body, ref_cleanup = body_projection(output(ref), case["source"], True)
            statuses["frame:" + ref_cleanup["status"]] += 1
            (directory / f"reference-{width}.body").write_text(ref_body)
            comparison = {"width": width, "reference": ref,
                          "referenceBodyProjection": ref_cleanup,
                          "content": compare(content, ref_body, cli_body, case["source"]),
                          "geometry": compare(geometry, ref_body, cli_body, case["source"])}
            for kind in ("content", "geometry"):
                statuses[kind + ":" + comparison[kind]["status"]] += 1
            row["comparisons"].append(comparison)
        if args.geometry_probe:
            row["geometryProbe"] = invoke([str(binaries["geometry"]), "--input", str(source),
                "--input-format", "roff", "--widths", ",".join(map(str, widths))], directory, "geometry", args.timeout)
            if row["geometryProbe"]["returncode"] != 0:
                statuses["process-error"] += 1
            try:
                probe = json.loads(output(row["geometryProbe"]))
                row["viewportAssertions"] = probe_assertions(probe, case, widths)
                statuses["viewport:" + row["viewportAssertions"]["status"]] += 1
                row["viewportComparisons"] = []
                for render in probe.get("renders", []):
                    reference = (directory / f"reference-{render['width']}.body").read_text()
                    viewport = "\n".join(r["text"] for r in render["rows"]) + "\n"
                    item = {"width": render["width"], "geometry": compare(geometry, reference, viewport, case["source"])}
                    statuses["viewport-geometry:" + item["geometry"]["status"]] += 1
                    row["viewportComparisons"].append(item)
            except (ValueError, KeyError, TypeError) as exc:
                row["viewportAssertions"] = {"status": "error", "reason": str(exc)}
                statuses["viewport:error"] += 1
        else:
            row["viewportAssertions"] = {"status": "uncovered", "reason": "No geometry probe supplied"}
            statuses["viewport:uncovered"] += 1
        save_json(directory / "report.json", row)
        rows.append({"id": case["id"], "sourceSha256": case["sourceSha256"], "report": str(directory / "report.json")})
        print(f"[{index + 1}/{len(selected)}] {case['id']}", flush=True)
    mutations = sensitivity_checks(content, geometry)
    save_json(args.output / "sensitivity.json", mutations)
    after = {k: file_evidence(v) for k, v in binaries.items()}
    stable = before == after
    comparator_stable = all(identity is None or file_evidence(Path(identity["path"])) == identity
                            for identity in (content_identity, geometry_identity))
    summary = {"schema": "mant-roff-behavior-matrix/1", "cases": len(rows),
               "historicalCases": sum(c["origin"].startswith("tests/") for c in selected),
               "generatedCases": sum(c["id"].startswith("generated/") for c in selected),
               "statuses": dict(statuses), "sensitivity": dict(Counter(x["status"] for x in mutations)),
               "binaryStable": stable, "comparatorStable": comparator_stable,
               "binariesAfter": after, "reports": rows,
               "status": "review-required", "limitations": [
                   "No whole-corpus or cross-platform claim; source matrix only.",
                   "CLI is unbounded; reference width is not equivalent to CLI soft wrapping.",
                   "Public target rows are checked on generated cases; exact target columns are not exposed.",
                   "Historical review text is provenance, never an automatic waiver."]}
    save_json(args.output / "summary.json", summary)
    print(json.dumps({k: v for k, v in summary.items() if k not in ("reports", "binariesAfter")}, indent=2))
    failed = not stable or not comparator_stable or any(k != "assertions:covered" and not k.endswith(":covered") for k in statuses)
    failed |= any(x["status"] != "detected" for x in mutations)
    hard_failure = not stable or not comparator_stable or statuses["process-error"] or any(k.endswith((":error", ":hard-failure")) for k in statuses)
    return 1 if hard_failure or (args.verify and failed) else 0


if __name__ == "__main__":
    raise SystemExit(main())
