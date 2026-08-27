#!/usr/bin/env python3
"""Validate the versioned, non-package mantdoc M0 conformance contract."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
MANIFESTS = ROOT / "tests" / "conformance" / "manifests" / "v1"
ACCEPTED_DIFFERENCES = ROOT / "tests" / "conformance" / "accepted-differences" / "v1.jsonl"

EXPECTED_SCHEMAS = {
    "oracle.toml": "mantdoc.oracle/v1",
    "capabilities.toml": "mantdoc.capabilities/v1",
    "corpora.toml": "mantdoc.corpora/v1",
    "differential.toml": "mantdoc.differential/v1",
    "baseline.toml": "mantdoc.baseline/v1",
    "legacy-api.toml": "mantdoc.legacy-api/v1",
    "m3-execution.toml": "mantdoc.m3-execution/v1",
}

EXPECTED_LEGACY_API_ITEMS = {
    "MacroSet", "NodeKind", "NormalizedListKind", "DisplayKind", "NormalizedFont",
    "AuthorMode", "NormalizedEnclosure", "TableAlignment", "TableCell", "NodeFlags",
    "Node", "Metadata", "Document", "DiagnosticLevel", "DiagnosticCode",
    "SourceLocation", "Diagnostic", "Diagnostic::code", "MAX_DECOMPRESSED_SOURCE_BYTES",
    "InputFormat", "IncludePolicy", "Compression", "ParseOptions", "ParseReport",
    "ParseErrorKind", "ParseError", "Parser", "Parser::new", "Parser::options",
    "Parser::with_input_format", "Parser::input_format", "Parser::with_mdoc_operating_system",
    "Parser::mdoc_operating_system", "Parser::parse_bytes", "Parser::parse_bundle",
    "Parser::parse_file", "MAX_SOURCE_BUNDLE_FILES", "MAX_SOURCE_BUNDLE_FILE_BYTES",
    "MAX_SOURCE_BUNDLE_BYTES", "SourceBundleErrorKind", "SourceBundleError",
    "SourceBundleError::path", "SourceBundleError::kind", "SourceBundle",
    "SourceBundle::new", "SourceBundle::len", "SourceBundle::is_empty",
    "SourceBundle::total_bytes", "SourceBundle::get", "SourceBundle::insert",
    "SpecialCharacter", "special_character", "LIBMANDOC_VERSION", "DEFAULT_RENDER_OUTPUT_BYTES",
    "DEFAULT_RENDER_WIDTH", "MAX_RENDER_OUTPUT_BYTES", "MIN_RENDER_WIDTH",
    "MAX_RENDER_WIDTH", "RenderFormat", "RenderReport", "RenderErrorKind", "RenderError",
    "Renderer", "Renderer::new", "Renderer::with_parser", "Renderer::with_width",
    "Renderer::with_max_output_bytes", "Renderer::with_html_fragment", "Renderer::parser",
    "Renderer::format", "Renderer::width", "Renderer::max_output_bytes",
    "Renderer::html_fragment", "Renderer::render_bytes", "Renderer::render_bundle",
    "Renderer::render_file",
}

CLASSIFICATIONS = {"direct", "redesigned", "optional", "removal"}
DIFFERENCE_FIELDS = {
    "id", "status", "layer", "classification", "oracle_id", "corpus_id", "case_id",
    "decompressed_source_sha256", "parser_config_fingerprint", "canonical_json_pointer",
    "reason", "evidence", "reviewed_at",
}


def fail(message: str) -> None:
    raise ValueError(message)


def load(name: str) -> dict[str, object]:
    path = MANIFESTS / name
    if not path.is_file():
        fail(f"missing manifest: {path.relative_to(ROOT)}")
    with path.open("rb") as handle:
        document = tomllib.load(handle)
    if document.get("schema") != EXPECTED_SCHEMAS[name]:
        fail(f"unsupported schema in {name}: {document.get('schema')!r}")
    return document


def unique(values: list[dict[str, object]], field: str, context: str) -> None:
    seen: set[object] = set()
    for value in values:
        identifier = value.get(field)
        if not isinstance(identifier, str) or not identifier:
            fail(f"{context} has a missing {field}")
        if identifier in seen:
            fail(f"duplicate {field} in {context}: {identifier}")
        seen.add(identifier)


def is_sha256(value: object) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def validate() -> None:
    documents = {name: load(name) for name in EXPECTED_SCHEMAS}
    oracle = documents["oracle.toml"]
    oracle_id = oracle.get("id")
    if not isinstance(oracle_id, str) or not oracle_id:
        fail("oracle.toml must contain a non-empty id")
    if oracle.get("workspace_commit") != "863d2b3867dfb38b7b4db265073a3d4b9b1a10e9":
        fail("oracle.toml must identify the frozen implementation baseline")
    if oracle.get("working_branch") != "codex/mantdoc-native-rewrite":
        fail("oracle.toml must identify the migration branch")
    patch_set = oracle.get("legacy", {}).get("patch_set", {})
    if patch_set.get("entry_count") != 28:
        fail("oracle must account for all 28 patched libmandoc files")
    behavior_patches = [
        patch
        for behavior in patch_set.get("required_behavior", [])
        for patch in behavior.get("patches", [])
    ]
    if len(behavior_patches) != 28 or len(set(behavior_patches)) != 28:
        fail("oracle patch behavior groups must cover each patch exactly once")

    for name, document in documents.items():
        if name != "oracle.toml" and document.get("oracle_id") != oracle_id:
            fail(f"{name} refers to a different oracle")

    api = documents["legacy-api.toml"]
    items = api.get("item", [])
    consumers = api.get("consumer", [])
    unique(items, "id", "legacy-api item")
    unique(consumers, "id", "legacy-api consumer")
    ids = {item["id"] for item in items}
    if ids != EXPECTED_LEGACY_API_ITEMS:
        missing = sorted(EXPECTED_LEGACY_API_ITEMS - ids)
        unexpected = sorted(ids - EXPECTED_LEGACY_API_ITEMS)
        fail(f"legacy API inventory drifted; missing={missing}, unexpected={unexpected}")
    for item in [*items, *consumers]:
        if not item.get("owner") or not item.get("destination", item.get("mantdoc")):
            fail(f"legacy API entry lacks owner or destination: {item.get('id')}")
        classification = item.get("classification")
        if classification is not None and classification not in CLASSIFICATIONS:
            fail(f"invalid classification for {item.get('id')}: {classification}")

    capabilities = documents["capabilities.toml"].get("capability", [])
    unique(capabilities, "id", "capability matrix")
    for capability in capabilities:
        if capability.get("classification") not in CLASSIFICATIONS:
            fail(f"invalid capability classification: {capability.get('id')}")
        for field in ("legacy", "consumers", "mantdoc_destination", "phase", "completion_gate", "acceptance"):
            if not capability.get(field):
                fail(f"capability lacks {field}: {capability.get('id')}")

    corpora = documents["corpora.toml"].get("corpus", [])
    unique(corpora, "id", "corpus manifest")
    corpus_by_id = {corpus["id"]: corpus for corpus in corpora}
    stable = corpus_by_id.get("mandoc-stable-1.14.6")
    head = corpus_by_id.get("mandoc-head-cf84231e")
    fixtures = corpus_by_id.get("fixed-roff-fixtures")
    if stable is None or stable.get("input_count") != 572 or stable.get("expected_output_count") != 1189:
        fail("stable mandoc corpus counts drifted")
    if stable.get("case_set_sha256") != "61739e43aa621dcfb57477da90d7b19cb5387bb5094e9c26249a7c35017da467":
        fail("stable mandoc corpus case-set identity drifted")
    if head is None or head.get("reference_commit") != "cf84231e26506943f1ef44249bf2ed248e339483":
        fail("HEAD corpus commit drifted")
    if head.get("regression_tree") != "94d7ef6de3556e769b10854caff71783ea53a7b5":
        fail("HEAD corpus regression subtree must stay pinned")
    head_case_manifest = ROOT / str(head.get("case_manifest", ""))
    if not head_case_manifest.is_file():
        fail("HEAD corpus must name an existing case manifest")
    head_cases = [
        json.loads(line)
        for line in head_case_manifest.read_text().splitlines()
        if line.strip()
    ]
    if not head_cases:
        fail("HEAD corpus case manifest must contain at least one reviewed case")
    for case in head_cases:
        for field in ("case_id", "pinned_commit", "blob_sha1", "source_sha256", "source_bytes", "focus", "license", "status"):
            if not case.get(field):
                fail(f"HEAD case lacks {field}: {case}")
        if case["pinned_commit"] != head["reference_commit"]:
            fail("HEAD case targets a different pinned commit")
        if not str(case["case_id"]).startswith("regress/usr.bin/mandoc/"):
            fail("HEAD case must remain inside the pinned mandoc regression tree")
    if fixtures is None or fixtures.get("real_roff_source_count") != 35:
        fail("fixed fixture corpus count drifted")
    m1_limits = documents["baseline.toml"].get("mantdoc_m1_limits", {})
    if m1_limits.get("max_source_lines") != 16_777_216:
        fail("M1 source-map line limit drifted")
    snapshot = documents["baseline.toml"].get("native_canonical_snapshot", {})
    if snapshot.get("schema") != "mantdoc.native-canonical-snapshot/v1":
        fail("native canonical snapshot schema drifted")
    snapshot_path_text = snapshot.get("path")
    if not isinstance(snapshot_path_text, str) or not snapshot_path_text:
        fail("native canonical snapshot must name its checked-in path")
    snapshot_path = ROOT / snapshot_path_text
    if not snapshot_path.is_file():
        fail(f"native canonical snapshot is missing: {snapshot_path_text}")
    if not isinstance(snapshot.get("command"), str) or "--verify" not in snapshot["command"]:
        fail("native canonical snapshot must publish its verify command")
    if snapshot.get("case_count") != stable["input_count"]:
        fail("native canonical snapshot case count must match the stable corpus")
    if not is_sha256(snapshot.get("records_sha256")):
        fail("native canonical snapshot has an invalid records hash")
    if not is_sha256(snapshot.get("file_sha256")):
        fail("native canonical snapshot has an invalid file hash")
    snapshot_bytes = snapshot_path.read_bytes()
    if hashlib.sha256(snapshot_bytes).hexdigest() != snapshot["file_sha256"]:
        fail("native canonical snapshot file hash drifted")
    try:
        snapshot_lines = snapshot_bytes.decode("utf-8").splitlines()
    except UnicodeDecodeError as error:
        fail(f"native canonical snapshot is not UTF-8: {error}")
    try:
        separator = snapshot_lines.index("")
    except ValueError:
        fail("native canonical snapshot must separate headers from records")
    header_lines = snapshot_lines[:separator]
    if any("=" not in line for line in header_lines) or len(header_lines) != 6:
        fail("native canonical snapshot header is malformed")
    headers = dict(line.split("=", 1) for line in header_lines)
    expected_headers = {
        "schema": snapshot["schema"],
        "corpus_id": "mandoc-stable-1.14.6",
        "oracle_id": oracle_id,
        "canonical_mdoc_os": "mantdoc canonical differential",
        "case_count": str(snapshot["case_count"]),
        "records_sha256": snapshot["records_sha256"],
    }
    if headers != expected_headers:
        fail("native canonical snapshot header disagrees with its manifest contract")
    records = snapshot_lines[separator + 1 :]
    if len(records) != snapshot["case_count"] or any(not line for line in records):
        fail("native canonical snapshot record count drifted")
    record_ids: set[str] = set()
    for record in records:
        parts = record.split("\t")
        if len(parts) != 3 or not parts[0].startswith("regress/") or not is_sha256(parts[1]) or not is_sha256(parts[2]):
            fail(f"native canonical snapshot has an invalid record: {record!r}")
        if parts[0] in record_ids:
            fail(f"native canonical snapshot has a duplicate case: {parts[0]}")
        record_ids.add(parts[0])
    records_hash = hashlib.sha256(
        "".join(f"{record}\n" for record in records).encode()
    ).hexdigest()
    if records_hash != snapshot["records_sha256"]:
        fail("native canonical snapshot records hash drifted")
    actual_fixture_files = [
        path
        for path in (ROOT / "tests" / "fixtures" / "roff" / "real").rglob("*")
        if path.is_file() and "LICENSES" not in path.parts
        and path.name not in {"README.md", "VERIFIED_TOPICS.txt"}
    ]
    if len(actual_fixture_files) != 35:
        fail(f"fixed fixture files disagree with manifest: {len(actual_fixture_files)}")

    m3_execution = documents["m3-execution.toml"]
    if m3_execution.get("corpus_id") != "mandoc-stable-1.14.6":
        fail("M3 execution gate must use the frozen stable mandoc corpus")
    m3_cases = m3_execution.get("case", [])
    unique(m3_cases, "id", "M3 execution cases")
    if len(m3_cases) != 29:
        fail("M3 execution gate must retain its 29 reviewed cases")
    for case in m3_cases:
        case_id = case.get("id")
        source_hash = case.get("source_sha256")
        if not isinstance(case_id, str) or not case_id.startswith("regress/roff/"):
            fail(f"M3 execution case must be a stable roff path: {case_id!r}")
        if not isinstance(source_hash, str) or len(source_hash) != 64 or any(
            character not in "0123456789abcdef" for character in source_hash
        ):
            fail(f"M3 execution case has invalid source hash: {case_id!r}")
        for field in ("ast_nodes", "expansion_steps"):
            if not isinstance(case.get(field), int) or case[field] < 0:
                fail(f"M3 execution case lacks non-negative {field}: {case_id!r}")
        if not isinstance(case.get("truncated"), bool):
            fail(f"M3 execution case lacks boolean truncation state: {case_id!r}")
        for diagnostic in case.get("diagnostics", []):
            if (
                not isinstance(diagnostic.get("code"), str)
                or not diagnostic["code"]
                or not isinstance(diagnostic.get("start"), int)
                or not isinstance(diagnostic.get("end"), int)
                or diagnostic["start"] < 0
                or diagnostic["end"] < diagnostic["start"]
            ):
                fail(f"M3 execution diagnostic is invalid: {case_id!r}")
    m3_by_id = {case["id"]: case for case in m3_cases}
    if m3_by_id.get("regress/roff/cond/close", {}).get("truncated") is not True:
        fail("M3 execution gate must retain cond/close's intentional truncated scope")
    indir_diagnostics = m3_by_id.get("regress/roff/de/indir", {}).get("diagnostics", [])
    if [diagnostic.get("code") for diagnostic in indir_diagnostics] != [
        "roff.undefined-reference",
        "roff.undefined-reference",
        "roff.empty-request",
        "roff.empty-request",
    ]:
        fail("M3 execution gate must retain de/indir's reviewed indirect-definition recovery")

    differential = documents["differential.toml"]
    required_difference_fields = set(differential["accepted_difference"]["required"])
    if required_difference_fields != DIFFERENCE_FIELDS:
        fail("accepted-difference required-field contract drifted")
    if ACCEPTED_DIFFERENCES.exists():
        for number, line in enumerate(ACCEPTED_DIFFERENCES.read_text().splitlines(), start=1):
            if not line.strip():
                continue
            entry = json.loads(line)
            missing = DIFFERENCE_FIELDS - set(entry)
            if missing:
                fail(f"accepted difference line {number} misses fields: {sorted(missing)}")
            if entry["oracle_id"] != oracle_id:
                fail(f"accepted difference line {number} targets a different oracle")

    print(
        "mantdoc conformance manifests valid: "
        f"{len(items)} legacy API items, {len(consumers)} consumers, "
        f"{len(capabilities)} capabilities, {len(corpora)} corpus lanes"
    )


if __name__ == "__main__":
    try:
        validate()
    except (OSError, ValueError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        print(f"mantdoc conformance manifest check failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
