#!/usr/bin/env python3
"""Record finite word/owner fixtures from the registered pristine oracle.

Run --check-sources without a local oracle to verify canonical exact inputs.
Run --check --evidence target/... to rerun all five reference profiles without
replacing fixtures. Full diagnostics, hashes and raw output stay below target.
No product executable participates in generating these expected results.
"""

import argparse
import collections
import concurrent.futures
import hashlib
import json
from pathlib import Path
from scripts.roff.fixtures import roff_fixture_reference
from scripts.roff.fixtures import roff_compatibility_cases
ROOT = Path(__file__).resolve().parents[3]
REFERENCE = ROOT / "target/mandoc-migration/reference/mandoc"
# These exact pristine inputs have a reproducible HTML SIGSEGV. Terminal,
# AST and lint still succeed. Keep their complete failure evidence; no HTML
# output or link gold is inferred from a failed reference profile.
HTML_SIGNAL_CASES = {
    "generated_body_rows": {
        f"link_{macro}_item_{state}"
        for macro in ("UR", "MT")
        for state in ("word_end", "bare_zero", "same_coordinate")
    },
    "output_owner_rows": {"link_UR_none_paragraph", "link_MT_none_paragraph"},
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def cells(line):
    """Decode terminal overstrike cells, preserving the accepted font bits."""
    output = []
    shadow = None
    for character in line:
        if character == "\b":
            shadow = output.pop() if output else None
            continue
        font = 0
        if shadow is not None:
            previous, old_font = shadow
            if previous == character:
                font = old_font | 1  # bold
            elif previous == "_":
                font = 2  # underline/italic
        output.append((" " if character == "\u00a0" else character, font))
        shadow = None
    return output


def projected_rows(text, family, eof):
    rows = [cells(line) for line in text.splitlines()]
    strings = ["".join(character for character, _ in row) for row in rows]
    headings = [row.strip() for row in strings]
    start = headings.index("DESCRIPTION") + 1
    if eof:
        end = next(index for index in range(start, len(headings))
                   if headings[index].startswith("Historical Oracle Footer"))
        if headings[end - 1] != "":
            raise ValueError("oracle EOF footer separator is missing")
        end -= 1
    else:
        end = headings.index("NEXT")
    if family == "kept_word_rows":
        # The KEEP contract includes the extra leading blank after A\p.
        # Remove only the shared manual margin, never per-row indentation.
        body = strings[start:end]
        margin = min((len(row) - len(row.lstrip(" ")) for row in body
                      if row.strip()), default=0)
        projected = [row[margin:].rstrip() if len(row) >= margin else ""
                     for row in body]
    else:
        # These word/row contracts do not claim terminal device origins.
        # Interior spaces and empty rows are retained exactly.
        projected = headings[start:end]
    styles = [font for row in rows[start:end] for character, font in row
              if character == "A"]
    return projected, styles


def record_profiles(family, name, source, selected=("ascii", "utf8", "html", "tree", "lint"), width=78):
    profiles = {}
    for profile in selected:
        completed = roff_fixture_reference.run_reference(
            REFERENCE, ["-T" + profile, f"-Owidth={width}",
             "-Ios=Historical Oracle Footer"],
            input_bytes=source.encode(), timeout=15,
            env={"PATH": "/usr/bin:/bin", "LC_ALL": "C.UTF-8", "TZ": "UTC"},
            check=False,
        )
        known_html_signal = (profile == "html" and completed.returncode == -11
                             and name in HTML_SIGNAL_CASES.get(family, set()))
        # lint records UNSUPP recovery diagnostics too. Rendering/AST must
        # succeed; a diagnostic-only nonzero status is retained as evidence.
        failed = completed.returncode < 0 or (profile != "lint" and completed.returncode >= 4)
        if failed and not known_html_signal:
            raise ValueError(f"oracle failed for {family}/{name}/{profile}")
        profiles[profile] = {
            "status": completed.returncode,
            "stdout": completed.stdout.decode("utf-8"),
            "stderr": completed.stderr.decode("utf-8"),
            "stdout_sha256": digest(completed.stdout),
            "stderr_sha256": digest(completed.stderr),
        }
    return profiles


def record(item):
    family, case = item
    profiles = record_profiles(family, case["name"], case["source"])
    rows, styles = projected_rows(profiles["utf8"]["stdout"], family, case.get("eof", False))
    # Keep the original native record in place; assertion-scope/fragment
    # metadata is appended without replacing its source or pristine rows.
    fixture = {key: value for key, value in case.items()
               if key not in {"scope", "reference_fragment", "wide_width"}}
    fixture["rows"] = rows
    if family == "generated_word_styles":
        fixture["accepted_a_styles"] = styles
    fixture.update({key: case[key] for key in ("scope", "reference_fragment", "wide_width") if key in case})
    full = {
        **fixture, "source_sha256": digest(case["source"].encode()),
        "profiles": profiles,
    }
    if fragment := case.get("reference_fragment"):
        fragment_profiles = record_profiles("table_macro_fragments", case["name"], fragment)
        if fragment_profiles["lint"]["status"] != 0:
            raise ValueError(f"table macro fragment has diagnostics: {case['name']}")
        fixture["fragment_rows"], _ = projected_rows(fragment_profiles["utf8"]["stdout"], family, False)
        full.update(fragment_rows=fixture["fragment_rows"],
                    reference_fragment_sha256=digest(fragment.encode()),
                    reference_fragment_profiles=fragment_profiles)
    if width := case.get("wide_width"):
        wide_profiles = record_profiles(family, case["name"], case["source"], ("utf8",), width)
        fixture["wide_rows"], _ = projected_rows(wide_profiles["utf8"]["stdout"], family, False)
        full.update(wide_rows=fixture["wide_rows"], wide_profiles=wide_profiles)
    return family, fixture, full


def check_sources(fixtures):
    for family, cases in roff_compatibility_cases.matrices():
        recorded = json.loads((fixtures / family / "cases.json").read_text())
        if len(cases) != roff_compatibility_cases.COUNTS[family] or len(cases) != len(recorded):
            raise ValueError(f"{family}: source count differs")
        for source, fixture in zip(cases, recorded):
            if any(fixture.get(key) != value for key, value in source.items()):
                raise ValueError(f"{family}/{source['name']}: canonical input differs")
        print(f"{family}: {len(cases)} canonical exact inputs match")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--fixtures", type=Path, default=ROOT / "crates/mant-engine/tests/roff_lowering/native_execution/fixtures")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--check-sources", action="store_true")
    args = parser.parse_args()
    if args.check_sources:
        check_sources(args.fixtures)
        return
    if args.evidence is None:
        parser.error("--evidence is required for complete reference recording")
    evidence = args.evidence.resolve()
    if not evidence.is_relative_to(ROOT / "target"):
        parser.error("stage evidence must stay below target")
    registration = roff_fixture_reference.verified_reference(ROOT, REFERENCE)
    evidence.mkdir(parents=True, exist_ok=True)
    tasks = [(family, case) for family, cases in roff_compatibility_cases.matrices() for case in cases]
    if collections.Counter(family for family, _ in tasks) != roff_compatibility_cases.COUNTS:
        raise ValueError("canonical matrix counts differ")
    candidates = collections.defaultdict(list)
    diagnostics = collections.defaultdict(collections.Counter)
    profile_failures = []
    with (evidence / "pristine-profiles.jsonl").open("w") as output:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
            for index, (family, fixture, full) in enumerate(pool.map(record, tasks), 1):
                candidates[family].append(fixture)
                diagnostics[family][str(full["profiles"]["lint"]["status"])] += 1
                profile_failures.extend(
                    {"family": family, "name": fixture["name"],
                     "profile": profile, "status": run["status"]}
                    for profile, run in full["profiles"].items()
                    if run["status"] < 0
                )
                output.write(json.dumps({"family": family, **full}, ensure_ascii=False) + "\n")
                if index % 400 == 0:
                    print(f"pristine inputs recorded: {index}/{len(tasks)}", flush=True)
    for family, cases in candidates.items():
        path = args.fixtures / family / "cases.json"
        recorded = json.dumps(cases, ensure_ascii=False, indent=2) + "\n"
        if args.check:
            if path.read_text() != recorded:
                raise ValueError(f"{family}: pristine output differs; fixtures were not changed")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(recorded)
    manifest = {
        "identity": registration["identity"],
        "reference_sha256": digest(REFERENCE.read_bytes()),
        "source_definitions_sha256": digest(Path(roff_compatibility_cases.__file__).read_bytes()),
        "counts": roff_compatibility_cases.COUNTS,
        "additional_reference_fragments": sum("reference_fragment" in case for _, case in tasks),
        "additional_wide_profiles": sum("wide_width" in case for _, case in tasks),
        "lint_status_counts": diagnostics,
        "reference_profile_failures": profile_failures,
        "expectations_from_product": False,
        "check_only": args.check,
    }
    (evidence / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
