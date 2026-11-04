"""Preserve source-bound corrections to the legacy synopsis row observer.

The original fixture remains immutable. These exact old sources independently
rerun all five pristine profiles; no ManT executable supplies expectations.
"""

import argparse
import copy
import hashlib
import json
from pathlib import Path

from scripts.roff.fixtures.reference_recipes import arguments, environment
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
DIRECTORY = ROOT / "crates/mant-engine/tests/roff_lowering/synopsis_execution"
LEGACY = DIRECTORY / "cases.json"
OBSERVATIONS = DIRECTORY / "word_observations.json"
UI_DIRECTORY = ROOT / "crates/mant-ui/src/document/tests/source_execution"
UI_LEGACY = UI_DIRECTORY / "cases.json"
UI_OBSERVATIONS = UI_DIRECTORY / "word_observations.json"
PROFILES = ("ascii", "utf8", "html", "tree", "lint")
IDENTIFIERS = tuple(
    f"direct-{macro}-{prefix}"
    for macro in ("fn", "fo")
    for prefix in ("ordinary", "zero-glyph", "pending-zero", "continuation")
) + tuple(
    f"pair-{macro}-blank-{blank}"
    for blank in ("literal", "escaped", "numbered", "motion", "pending-zero", "pending-break")
    for macro in ("fn", "fo")
)


def digest(value):
    return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()


def native_rows(raw):
    rows = []
    for row in raw.decode().splitlines():
        cells = []
        for character in row:
            if character == "\b":
                if cells:
                    cells.pop()
            else:
                cells.append(character)
        rows.append("".join(cells))
    start = rows.index("SYNOPSIS") + 1
    end = rows.index("NEXT")
    selected = rows[start:end]
    if selected[-1] != "":
        raise ValueError("missing known section separator")
    selected.pop()
    origin = min(len(row) - len(row.lstrip(" ")) for row in selected if row)
    return [row[origin:].rstrip(" ") if row else row for row in selected]


def record(reference, legacy, evidence):
    observations = []
    for case in legacy["cases"]:
        if case["id"] not in IDENTIFIERS:
            continue
        source = case["source"].encode()
        if digest(source) != case["source_sha256"]:
            raise ValueError("changed legacy source: " + case["id"])
        profiles = {}
        rows = None
        for profile in PROFILES:
            run = run_reference(reference, arguments(profile), input_bytes=source, env=environment())
            receipt = dict(status=run.returncode, stdout_sha256=digest(run.stdout),
                           stderr_sha256=digest(run.stderr), stderr=run.stderr.decode())
            # Legacy lint used a differently named input transport; its raw
            # receipt stays bound verbatim below. The new run records its own
            # diagnostic bytes, without rewriting filenames to match hashes.
            original = case["oracle"][profile]
            if (profile != "lint" and receipt != original
                    or profile == "lint" and receipt["status"] != original["status"]):
                raise ValueError(f"old pristine receipt changed: {case['id']}/{profile}")
            profiles[profile] = receipt
            if profile == "utf8":
                rows = native_rows(run.stdout)
            if evidence:
                directory = evidence / case["id"]
                directory.mkdir(parents=True, exist_ok=True)
                (directory / "source.roff").write_bytes(source)
                (directory / f"{profile}.stdout").write_bytes(run.stdout)
                (directory / f"{profile}.stderr").write_bytes(run.stderr)
        if [row.strip(" ") for row in rows] != case["rows"]:
            raise ValueError("correction is not solely the old leading-space observer: " + case["id"])
        suffix = next(row for row in rows if row.strip(" ") == "SUFFIX")
        if suffix != " SUFFIX":
            raise ValueError("unreviewed suffix observation: " + case["id"])
        reading = rows.copy()
        for blank in case["generated_reading_spaces"]:
            scalars = list(reading[blank["row"]])
            start, count = blank["start"], blank["count"]
            if scalars[start:start + count] != ["\u00a0"] * count:
                raise ValueError("changed authored/generated blank witness")
            scalars[start:start + count] = [" "] * count
            reading[blank["row"]] = "".join(scalars)
        portable = [suffix if row == "SUFFIX" else row for row in case["portable_rows"]]
        owners = copy.deepcopy(case["portable_owners"])
        if owners[-1]["rows"] != ["SUFFIX"]:
            raise ValueError("unreviewed original owner")
        owners[-1]["rows"] = [suffix]
        observations.append(dict(id=case["id"], source=case["source"], source_sha256=case["source_sha256"],
                                 original_profiles=case["oracle"], profiles=profiles,
                                 native_rows=rows, reading_rows=reading,
                                 portable_rows=portable, portable_owners=owners))
    if {case["id"] for case in observations} != set(IDENTIFIERS):
        raise ValueError("missing reviewed source")
    return dict(oracle_sha256=digest(reference.read_bytes()),
                original_fixture_sha256=digest(LEGACY.read_bytes()), cases=observations)



def ui_observations(recorded):
    """Bind the UI cohort's eight identical direct sources to the fresh runs."""
    legacy = json.loads(UI_LEGACY.read_bytes())
    by_id = {case["id"]: case for case in legacy["cases"]}
    observations = []
    for case in recorded["cases"]:
        if not case["id"].startswith("direct-"):
            continue
        old = by_id[case["id"]]
        if old["source"] != case["source"]:
            raise ValueError("UI source differs from its independent native run")
        if old["rows"] != [row.strip(" ") for row in case["native_rows"]]:
            raise ValueError("unreviewed UI native row correction")
        owners = ["\n".join(owner["rows"]) for owner in case["portable_owners"]]
        if [text.strip(" ") for text in owners] != old["portable_owner_texts"]:
            raise ValueError("unreviewed UI owner correction")
        observations.append(dict(
            id=case["id"], source=case["source"], source_sha256=case["source_sha256"],
            profiles=case["profiles"], native_rows=case["native_rows"],
            reading_rows=case["reading_rows"], portable_rows=case["portable_rows"],
            portable_owner_texts=owners))
    if len(observations) != 8:
        raise ValueError("missing direct UI source observations")
    return dict(oracle_sha256=recorded["oracle_sha256"],
                original_fixture_sha256=digest(UI_LEGACY.read_bytes()), cases=observations)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--evidence", type=Path)
    options = parser.parse_args()
    reference = ROOT / "target/mandoc-migration/reference/mandoc"
    verified_reference(ROOT, reference)
    value = record(reference, json.loads(LEGACY.read_bytes()), options.evidence)
    ui = ui_observations(value)
    if options.write:
        OBSERVATIONS.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")
        UI_OBSERVATIONS.write_text(json.dumps(ui, indent=2, ensure_ascii=False) + "\n")
    elif value != json.loads(OBSERVATIONS.read_bytes()):
        raise ValueError("source-bound synopsis observations changed")
    elif ui != json.loads(UI_OBSERVATIONS.read_bytes()):
        raise ValueError("source-bound UI observations changed")
    print(f"verified {len(value['cases'])} source-bound observations / 100 pristine profiles")


if __name__ == "__main__":
    main()
