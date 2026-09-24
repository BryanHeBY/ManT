#!/usr/bin/env python3
"""Source-bound explanation gold for the annotated Fixed product path.

This deliberately does not reuse the Flow profiler or its AST/ContentStore
reader. A missing/drifted source or malformed Fixed response is unresolved,
never a passing comparison. Expectations are manually reviewed, not learned
from the current renderer.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

from roff_audit_common import source_digest


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = "mant.annotated-fixed-query-gold/v1"
RESULT_SCHEMA = "mant.annotated-fixed-query-gold-result/v1"
CLASSES = ("directEntry", "relatedEntry", "entryMention", "contextMention")
WIRE_CLASSES = ("direct-entry", "related-entry", "entry-mention", "context-mention")
MAX_COPY_BYTES = 4_194_304
MAX_PARTS = 1024
MAX_COLUMN = 1_048_576


def checked_parts(selection: object) -> tuple[list[dict], list[dict]]:
    if not isinstance(selection, dict):
        raise ValueError("Fixed selection is not an object")
    parts, joins = selection.get("parts"), selection.get("joins")
    if (not isinstance(parts, list) or not isinstance(joins, list)
            or len(parts) > MAX_PARTS or len(joins) != max(0, len(parts) - 1)):
        raise ValueError("invalid Fixed selection part/join count")
    previous = None
    for part in parts:
        if not isinstance(part, dict) or not isinstance(part.get("text"), str):
            raise ValueError("invalid Fixed selection part")
        slice_ = part.get("slice")
        if not isinstance(slice_, dict) or not all(type(slice_.get(field)) is int for field in (
            "run", "startByte", "endByte"
        )) or not all(type(part.get(field)) is int for field in (
            "row", "runColumn", "column", "width"
        )) or not (slice_["run"] > 0 and 0 <= slice_["startByte"] < slice_["endByte"]
                  and slice_["endByte"] - slice_["startByte"] == len(part["text"].encode("utf-8"))
                  and 0 < part["row"] <= MAX_COLUMN
                  and 0 <= part["runColumn"] <= part["column"] <= MAX_COLUMN
                  and 0 <= part["width"] <= MAX_COLUMN
                  and part["column"] + part["width"] <= MAX_COLUMN):
            raise ValueError("invalid Fixed slice or geometry")
        if previous is not None:
            old = previous["slice"]
            if (slice_["run"] < old["run"]
                    or (slice_["run"] == old["run"]
                        and slice_["startByte"] < old["endByte"])
                    or part["row"] < previous["row"]
                    or (part["row"] == previous["row"]
                        and part["column"] < previous["column"] + previous["width"])):
                raise ValueError("unordered or overlapping Fixed slice")
        previous = part
    for join in joins:
        if not isinstance(join, dict) or join.get("kind") not in (
            "direct-contact", "authored-separator", "hard-boundary", "unknown"
        ):
            raise ValueError("invalid Fixed text join")
        if join["kind"] == "authored-separator" and (
            not isinstance(join.get("text"), str) or not join["text"]
            or set(join["text"]) != {" "}
        ):
            raise ValueError("invalid Fixed authored separator")
    return parts, joins


def complete_form(selection: object) -> str:
    """Join only native-proven logical continuity, never screen adjacency."""
    parts, joins = checked_parts(selection)
    text = ""
    for index, part in enumerate(parts):
        if not isinstance(part, dict) or not isinstance(part.get("text"), str):
            raise ValueError("invalid Fixed form part")
        if index:
            join = joins[index - 1]
            if not isinstance(join, dict):
                raise ValueError("invalid Fixed form join")
            if join.get("kind") == "authored-separator":
                separator = join.get("text")
                if not isinstance(separator, str) or not separator or set(separator) != {" "}:
                    raise ValueError("invalid authored separator")
                text += separator
            elif join.get("kind") != "direct-contact":
                raise ValueError("form lacks proven logical continuity")
        text += part["text"]
    return text


def displayed_body(selection: object) -> str:
    """Read final physical rows/cells without treating TextJoin as layout."""
    parts, _ = checked_parts(selection)
    rows: list[str] = []
    row, end_column = 0, 0
    copied = 0
    for part in parts:
        if part["row"] < row or (part["row"] == row and part["column"] < end_column):
            raise ValueError("unordered Fixed body part")
        if part["row"] != row:
            gap = part["row"] - row
            copied += gap
            if copied > MAX_COPY_BYTES:
                raise ValueError("Fixed body padding exceeds copy budget")
            rows.append("\n" * gap)
            row, end_column = part["row"], 0
        gap = part["column"] - end_column
        copied += gap + len(part["text"].encode("utf-8"))
        if copied > MAX_COPY_BYTES:
            raise ValueError("Fixed body exceeds copy budget")
        rows.append(" " * gap)
        rows.append(part["text"])
        end_column = part["column"] + part["width"]
    return "".join(rows)


def fixed_context(evidence: dict, supports: object) -> dict | None:
    reference = evidence.get("support")
    if reference is None:
        return None
    if (type(reference) is not int or not isinstance(supports, list)
            or not 0 <= reference < len(supports)):
        raise ValueError("Fixed owner has an invalid support index")
    support = supports[reference]
    if not isinstance(support, dict) or support.get("kind") != "fixed-declaration-group":
        raise ValueError("Fixed owner references a non-Fixed context")
    members = support.get("members")
    if not isinstance(members, list) or not 2 <= len(members) <= 256:
        raise ValueError("Fixed group has invalid member count")
    heads, seen = [], set()
    matched = False
    for member in members:
        if not isinstance(member, dict) or type(member.get("key")) is not int or member["key"] < 1:
            raise ValueError("Fixed group has an invalid owner key")
        node = member.get("outline", {}).get("node", {})
        if not isinstance(node, dict) or not isinstance(node.get("path"), str):
            raise ValueError("Fixed group has an invalid member trail")
        identity = (member["key"], node["path"])
        if identity in seen:
            raise ValueError("Fixed group repeats an owner")
        seen.add(identity)
        heads.append(complete_form(member.get("head")))
        if (member["key"] == evidence.get("content", {}).get("key")
                and member["outline"] == evidence.get("outline")):
            matched = True
    if not matched:
        raise ValueError("Fixed support does not contain its referring owner")
    body = displayed_body(support.get("readingBody"))
    if not body.strip():
        raise ValueError("Fixed group has no provider body")
    return {"heads": heads, "body": " ".join(body.split())}


def owner_record(evidence: object, requested: str, supports: object) -> dict:
    if not isinstance(evidence, dict) or evidence.get("class") != "direct-entry":
        raise ValueError("not a Fixed direct owner")
    entry, content = evidence.get("entry"), evidence.get("content")
    if not isinstance(entry, dict) or not isinstance(content, dict) or content.get("kind") != "fixed-owner":
        raise ValueError("Fixed direct owner has no facts or reading body")
    if entry.get("forms") != [] or not isinstance(entry.get("fixedForms"), list):
        raise ValueError("Fixed owner contains Flow or missing forms")
    kind = entry.get("kind")
    if not isinstance(kind, dict):
        raise ValueError("Fixed owner has no kind")
    body = displayed_body(content.get("readingBody"))
    node = evidence.get("outline", {}).get("node", {})
    if not isinstance(node, dict) or not all(isinstance(node.get(field), str) and node[field] for field in ("id", "path")):
        raise ValueError("Fixed owner lacks outline identity")
    bases = evidence.get("bases")
    if not isinstance(bases, list) or not bases or not all(isinstance(basis, dict) and isinstance(basis.get("kind"), str) for basis in bases):
        raise ValueError("Fixed direct owner lacks evidence bases")
    forms = [complete_form(form) for form in entry["fixedForms"]]
    direct_match = False
    for basis in bases:
        basis_kind = basis["kind"]
        if basis_kind == "name":
            matches = basis.get("matches")
            if not isinstance(matches, list) or not matches or not all(
                isinstance(match, dict) and match.get("name") == requested
                and match["name"] in entry.get("names", []) for match in matches
            ):
                raise ValueError("Fixed name basis does not bind the request")
            direct_match = True
        elif basis_kind == "form":
            matches = basis.get("matches")
            if not isinstance(matches, list) or not matches or not all(
                isinstance(match, dict) and match.get("text") == requested
                and type(match.get("sourceFormIndex")) is int
                and 0 <= match["sourceFormIndex"] < len(forms)
                and forms[match["sourceFormIndex"]] == requested for match in matches
            ):
                raise ValueError("Fixed form basis does not bind the request")
            direct_match = True
        elif basis_kind == "identity":
            fields = basis.get("fields")
            if not isinstance(fields, list) or not fields or not all(
                field in ("id", "path") and node[field] == requested for field in fields
            ):
                raise ValueError("Fixed identity basis does not bind the request")
            direct_match = True
    if not direct_match:
        raise ValueError("Fixed direct owner has no bound request basis")
    return {
        "source": evidence.get("source"),
        "id": node.get("id"),
        "path": node.get("path"),
        "kind": kind.get("parameterKind", kind.get("kind")),
        "names": entry.get("names"),
        "forms": forms,
        "body": " ".join(body.split()),
        "emptyDescription": not body.strip(),
        "aliasGroups": entry.get("aliasGroups"),
        "aliasOf": entry.get("aliasOf"),
        "basisKinds": [basis["kind"] for basis in bases],
        "omitted": any(evidence.get(field, False) for field in (
            "supportOmitted", "previewsOmitted", "detailsOmitted",
            "matchDetailsOmitted", "nameBindingsOmitted", "contentOmitted")),
        "readingContext": fixed_context(evidence, supports),
    }


def compare(probe: dict, response: object) -> tuple[str, list[str], list[dict]]:
    try:
        if not isinstance(response, dict) or response.get("schema") != "mant.explanation/v0.12":
            raise ValueError("missing explanation schema")
        counts = response["counts"]
        evidence = response["evidence"]
        if not isinstance(evidence, list):
            raise ValueError("invalid evidence array")
        if not all(isinstance(item, dict) and item.get("class") in WIRE_CLASSES for item in evidence):
            raise ValueError("invalid evidence class")
        actual = [owner_record(item, probe["query"], response.get("supports")) for item in evidence
                  if item.get("class") == "direct-entry"]
        totals = {kind: counts[kind]["total"] for kind in CLASSES}
        if not all(type(total) is int and total >= 0 for total in totals.values()):
            raise ValueError("invalid evidence counts")
        observed = Counter(item["class"] for item in evidence)
        if (type(response.get("total")) is not int
                or type(response.get("returned")) is not int
                or response["total"] != sum(totals.values())
                or response["returned"] != len(evidence)
                or response.get("nextOffset") is not None
                or any(counts[kind]["returned"] != observed[wire]
                       or totals[kind] != observed[wire]
                       for kind, wire in zip(CLASSES, WIRE_CLASSES, strict=True))):
            raise ValueError("incomplete or inconsistent Fixed evidence pagination")
        truncation = response.get("truncation")
        if not isinstance(truncation, dict) or truncation.get("candidates") or truncation.get("relations"):
            raise ValueError("Fixed candidate or relation traversal is truncated")
        if response.get("outcome") != ("evidence" if evidence else "no-evidence"):
            raise ValueError("Fixed outcome contradicts evidence")
    except (ValueError, KeyError, TypeError, AttributeError, UnicodeError) as error:
        return "unresolved", [str(error)], []
    expected = probe.get("expected")
    if not isinstance(expected, list) or not probe.get("review"):
        return "unresolved", ["probe has no reviewed Fixed expectation"], actual
    errors = []
    if truncation.get("content"):
        errors.append("Fixed content budget omitted selected evidence")
    if totals["directEntry"] != len(expected) or len(actual) != len(expected):
        errors.append(f"direct owners: expected {len(expected)}, total={totals['directEntry']}, returned={len(actual)}")
    for kind, count in probe.get("expectedCounts", {}).items():
        if kind not in CLASSES or type(count) is not int:
            return "unresolved", ["invalid expected count"], actual
        if totals[kind] != count:
            errors.append(f"{kind}: {totals[kind]} != {count}")
    used = set()
    for want in expected:
        source = {**want["source"], "source": want["source"].get("source", 1)}
        matches = [(index, got) for index, got in enumerate(actual)
                   if index not in used and got["source"] == source and got["forms"] == want["forms"]]
        if len(matches) != 1:
            errors.append(f"source {source}: expected one owner with forms {want['forms']!r}, found {len(matches)}")
            continue
        index, got = matches[0]
        used.add(index)
        for field in ("kind", "names", "forms", "emptyDescription"):
            if got[field] != want[field]:
                errors.append(f"{source} {field}: {got[field]!r} != {want[field]!r}")
        for field in ("id", "path"):
            if field in want and got[field] != want[field]:
                errors.append(f"{source} {field}: {got[field]!r} != {want[field]!r}")
        if not set(got["basisKinds"]) & {"name", "form", "identity"}:
            errors.append(f"{source}: direct owner has no direct declaration or identity basis")
        if "basisKinds" in want and got["basisKinds"] != want["basisKinds"]:
            errors.append(f"{source}: bases {got['basisKinds']!r} != {want['basisKinds']!r}")
        if got["omitted"]:
            errors.append(f"{source}: selected facts/body omitted")
        if got["aliasGroups"] or got["aliasOf"]:
            errors.append(f"{source}: unreviewed alias relation")
        if "readingContext" in want:
            context = got["readingContext"]
            requested_context = want["readingContext"]
            if requested_context is None:
                if context is not None:
                    errors.append(f"{source}: unexpected reading context")
            elif context is None:
                errors.append(f"{source}: missing reading context")
            else:
                if context["heads"] != requested_context["heads"]:
                    errors.append(f"{source}: wrong reading-context members")
                for witness in requested_context.get("bodyIncludes", []):
                    if witness not in context["body"]:
                        errors.append(f"{source}: missing reading-context witness {witness!r}")
        for witness in want.get("bodyIncludes", []):
            if witness not in got["body"]:
                errors.append(f"{source}: missing body witness {witness!r}")
        for witness in want.get("bodyExcludes", []):
            if witness in got["body"]:
                errors.append(f"{source}: wrong-owner body witness {witness!r}")
    return ("failure" if errors else "passed"), errors, actual


def fetch_pages(cli: Path, path: Path, query: str, timeout: int) -> dict:
    """Collect exact class-order pages, checking each page before comparison."""
    offset = 0
    first = None
    evidence = []
    while True:
        command = [str(cli), "--annotated-preview", "--input", str(path),
                   "--input-format", "roff", "--explain", query,
                   "--limit", "16", "--offset", str(offset),
                   "--explain-content-bytes", "4194304", "--format", "json", "--compact"]
        completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
        if completed.returncode:
            raise ValueError(f"CLI exit {completed.returncode}: {completed.stderr.strip()[:300]}")
        page = json.loads(completed.stdout)
        if not isinstance(page, dict) or page.get("schema") != "mant.explanation/v0.12":
            raise ValueError("missing explanation schema")
        query_record = page.get("query")
        options = query_record.get("options") if isinstance(query_record, dict) else None
        if (not isinstance(options, dict) or query_record.get("entry") != query
                or options.get("offset") != offset or options.get("limit") != 16
                or options.get("contentBytes") != MAX_COPY_BYTES):
            raise ValueError("Fixed response does not echo the requested page")
        items = page.get("evidence")
        if (not isinstance(items, list) or type(page.get("returned")) is not int
                or type(page.get("total")) is not int
                or page["returned"] != len(items) or len(items) > 16):
            raise ValueError("invalid Fixed page length")
        if not all(isinstance(item, dict) and item.get("class") in WIRE_CLASSES for item in items):
            raise ValueError("invalid Fixed evidence class")
        counts = page.get("counts")
        if not isinstance(counts, dict) or any(
            not isinstance(counts.get(kind), dict)
            or type(counts[kind].get("total")) is not int
            or type(counts[kind].get("returned")) is not int
            or counts[kind]["total"] < counts[kind]["returned"]
            or counts[kind]["returned"] != sum(item["class"] == wire for item in items)
            for kind, wire in zip(CLASSES, WIRE_CLASSES, strict=True)
        ) or page.get("total") != sum(counts[kind]["total"] for kind in CLASSES):
            raise ValueError("Fixed page counts disagree with returned evidence")
        truncation = page.get("truncation")
        if (not isinstance(truncation, dict)
                or truncation.get("candidates") or truncation.get("relations")):
            raise ValueError("Fixed page has incomplete candidate or relation traversal")
        if first is None:
            first = page
        elif (page.get("total") != first.get("total")
              or any(page["counts"][kind]["total"] != first["counts"][kind]["total"]
                     for kind in CLASSES)
              or any(page.get(field) != first.get(field)
                     for field in ("sourceContext", "producer", "semanticsComplete"))):
            raise ValueError("Fixed page totals or source context changed")
        if any(type(item.get("ordinal")) is not int
               or item["ordinal"] != offset + index for index, item in enumerate(items)):
            raise ValueError("Fixed page has noncontiguous ordinals")
        evidence.extend(items)
        if page.get("truncation", {}).get("content"):
            first["truncation"]["content"] = True
        following = page.get("nextOffset")
        if following is None:
            break
        if type(following) is not int or following != offset + len(items) or following <= offset:
            raise ValueError("Fixed page has invalid nextOffset")
        offset = following
        if len(evidence) > 10_000:
            raise ValueError("Fixed gold exceeds candidate bound")
    assert first is not None
    first["evidence"] = evidence
    first["returned"] = len(evidence)
    first["counts"] = {kind: {"total": first["counts"][kind]["total"],
                              "returned": sum(item["class"] == wire for item in evidence)}
                       for kind, wire in zip(CLASSES, WIRE_CLASSES, strict=True)}
    first.pop("nextOffset", None)
    return first


def run(manifest: Path, cli: Path, timeout: int, only: set[str]) -> dict:
    panel = json.loads(manifest.read_text(encoding="utf-8"))
    if panel.get("schema") != SCHEMA:
        raise ValueError("unsupported Fixed query gold schema")
    roots = panel.get("roots", {})
    results = []
    available = {source["id"] for source in panel["sources"]}
    if only - available:
        raise ValueError(f"unknown Fixed gold source IDs: {sorted(only - available)}")
    for source in panel["sources"]:
        key = source["id"]
        if only and key not in only:
            continue
        path = Path(roots[source["root"]]) / source["path"]
        if not path.is_absolute():
            path = manifest.parent / path
        path = path.resolve()
        if source_digest(path) != source["sha256"]:
            results.append({"id": key, "status": "unresolved", "reasons": ["source missing or SHA mismatch"]})
            continue
        for probe in source["queries"]:
            try:
                response = fetch_pages(cli, path, probe["query"], timeout)
                status, reasons, actual = compare(probe, response)
            except (OSError, subprocess.TimeoutExpired, json.JSONDecodeError, ValueError) as error:
                status, reasons, actual = "unresolved", [str(error)], []
            results.append({"id": key, "query": probe["query"], "status": status,
                            "reasons": reasons, "actual": actual})
    if not results:
        raise ValueError("Fixed query gold selected no sources")
    return {"schema": RESULT_SCHEMA, "manifest": str(manifest), "results": results}


def self_check() -> None:
    selection = {"parts": [{"slice": {"run": 1, "startByte": 0, "endByte": 2},
                             "text": "-a", "row": 1, "runColumn": 0, "column": 0, "width": 2}], "joins": []}
    evidence = {"class": "direct-entry", "source": {"source": 1, "line": 4, "column": 2},
                "outline": {"node": {"id": "x", "path": "1/e1"}},
                "bases": [{"kind": "name", "matches": [{"name": "-a"}]}],
                "entry": {"kind": {"kind": "parameter", "parameterKind": "option"},
                          "names": ["-a"], "forms": [], "fixedForms": [selection],
                          "aliasGroups": []},
                "content": {"kind": "fixed-owner", "readingBody": {
                    "parts": [{"slice": {"run": 2, "startByte": 0, "endByte": 4},
                               "text": "BODY", "row": 2, "runColumn": 4, "column": 4,
                               "width": 4}], "joins": []}}}
    response = {"schema": "mant.explanation/v0.12", "counts": {
        kind: {"total": int(kind == "directEntry"),
               "returned": int(kind == "directEntry")} for kind in CLASSES},
        "total": 1, "returned": 1, "outcome": "evidence",
        "truncation": {"candidates": False, "relations": False,
                       "content": False}, "evidence": [evidence]}
    want = {"source": {"line": 4, "column": 2}, "kind": "option", "names": ["-a"],
            "forms": ["-a"], "emptyDescription": False, "bodyIncludes": ["BODY"]}
    probe = {"query": "-a", "review": "source and fixed output checked", "expected": [want]}
    assert compare(probe, response)[0] == "passed"
    assert compare({**probe, "expected": []}, response)[0] == "failure"
    assert compare({**probe, "expected": [{**want, "bodyIncludes": ["OTHER"]}]}, response)[0] == "failure"
    assert compare(probe, {**response, "evidence": [{**evidence, "contentOmitted": True}]})[0] == "failure"
    assert compare(probe, {**response, "counts": {**response["counts"],
        "directEntry": {"total": 1, "returned": 0}}})[0] == "unresolved"
    assert compare(probe, {**response, "total": True})[0] == "unresolved"
    assert compare(probe, {**response, "returned": True})[0] == "unresolved"
    assert compare(probe, {**response, "evidence": [{**evidence,
        "bases": [{"kind": "name", "matches": []}]}]})[0] == "unresolved"
    oversized = {**selection["parts"][0], "column": MAX_COLUMN + 1}
    assert compare(probe, {**response, "evidence": [{**evidence, "entry": {
        **evidence["entry"], "fixedForms": [{"parts": [oversized], "joins": []}]}}]})[0] == "unresolved"
    backward = [{**selection["parts"][0], "slice": {"run": 10, "startByte": 0, "endByte": 2}},
                {**selection["parts"][0], "slice": {"run": 9, "startByte": 0, "endByte": 2},
                 "column": 2}]
    assert compare(probe, {**response, "evidence": [{**evidence, "entry": {
        **evidence["entry"], "fixedForms": [{"parts": backward,
                                              "joins": [{"kind": "direct-contact"}]}]}}]})[0] == "unresolved"
    broken = {**evidence, "entry": {**evidence["entry"], "fixedForms": [{
        "parts": selection["parts"] * 2, "joins": [{"kind": "unknown"}]}]}}
    assert compare(probe, {**response, "evidence": [broken]})[0] == "unresolved"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=ROOT / "tests/fixtures/roff/ANNOTATED_FIXED_QUERY_GOLD.json")
    parser.add_argument("--cli", type=Path, default=ROOT / "target/debug/mant")
    parser.add_argument("--source", action="append", default=[], help="select a source ID")
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument("--json", action="store_true", help="write a new unique report under target/annotated-fixed-query-gold")
    parser.add_argument("--self-check", action="store_true")
    args = parser.parse_args()
    if args.self_check:
        self_check()
        print("annotated Fixed query gold self-check succeeded")
        return 0
    try:
        if args.timeout < 1 or not args.cli.is_file():
            raise ValueError("build the annotated-preview CLI and provide a positive timeout")
        result = run(args.manifest, args.cli.resolve(), args.timeout, set(args.source))
        if args.json:
            target = ROOT / "target/annotated-fixed-query-gold"
            if target.is_symlink():
                raise ValueError("Fixed gold report directory must not be a symlink")
            target.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", suffix=".json",
                                             prefix="report-", dir=target, delete=False) as output:
                output.write(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
                print(f"report: {output.name}")
    except (OSError, KeyError, TypeError, ValueError) as error:
        print(f"annotated Fixed query gold: {error}", file=sys.stderr)
        return 2
    counts = Counter(row["status"] for row in result["results"])
    print("annotated Fixed query gold: " + json.dumps(counts, sort_keys=True))
    for row in result["results"]:
        if row["status"] != "passed":
            print(f"  {row['id']} {row.get('query', '')}: {row['status']}: {'; '.join(row['reasons'])}")
    return int(bool(counts["failure"] or counts["unresolved"]))


if __name__ == "__main__":
    raise SystemExit(main())
