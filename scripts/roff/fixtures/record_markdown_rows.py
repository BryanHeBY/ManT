"""Record/check Markdown hard-row roff inputs using the locked pristine oracle.

Only independent ASCII/UTF-8/HTML/tree/lint profiles supply expectations. The
recorder never invokes a product binary or reads a candidate rendering.
"""

import argparse
import hashlib
import json
from pathlib import Path

from scripts.roff.fixtures import acceptance_regions, roff_fixture_reference
from scripts.roff.fixtures.markdown_rule_cases import cases

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/markdown_hard_rows/cases.json"
PROFILES = (
    ("ascii", ("-Tascii", "-Owidth=78")),
    ("utf8", ("-Tutf8", "-Owidth=78")),
    ("html", ("-Thtml",)), ("tree", ("-Ttree",)), ("lint", ("-Tlint",)),
)


def sha(value):
    return hashlib.sha256(value).hexdigest()


def record(check, evidence):
    binary = ROOT / "target/mandoc-migration/reference/mandoc"
    registration = roff_fixture_reference.verified_reference(ROOT, binary)
    recorded = {"oracle": {"identity": registration["identity"],
                           "sha256": sha(binary.read_bytes())}, "cases": []}
    raw = []
    for case in cases():
        source = case["source"].encode()
        if sha(source) != case["source_sha256"]:
            raise ValueError(f"{case['id']}: source identity mismatch")
        profiles = {}
        for name, arguments in PROFILES:
            process = roff_fixture_reference.run_reference(
                binary, arguments, input_bytes=source, check=False)
            if process.returncode:
                raise ValueError(f"{case['id']}: native {name} failed: {process.stderr!r}")
            profiles[name] = {"status": process.returncode,
                              "stdout": process.stdout.decode("utf-8"),
                              "stderr": process.stderr.decode("utf-8"),
                              "stdout_sha256": sha(process.stdout),
                              "stderr_sha256": sha(process.stderr)}
        region = acceptance_regions.select_native_region(
            profiles["utf8"]["stdout"], profiles["tree"]["stdout"])
        if region["status"] != "asserted":
            raise ValueError(f"{case['id']}: reference region not qualified: {region}")
        recorded["cases"].append(dict(
            case, native_rows=region["rows"],
            profiles={name: {key: value for key, value in profile.items()
                             if key not in ("stdout", "stderr")}
                      for name, profile in profiles.items()}))
        raw.append({"case": case, "profiles": profiles, "region": region})
    evidence = evidence.resolve()
    if not evidence.is_relative_to(ROOT / "target"):
        raise ValueError("native recording evidence must remain beneath target")
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / "native-profiles.json").write_text(
        json.dumps(raw, ensure_ascii=False, indent=2) + "\n")
    if check:
        if recorded != json.loads(FIXTURE.read_text()):
            raise ValueError("hard-row fixture differs from the exact pristine runs")
    else:
        FIXTURE.write_text(json.dumps(recorded, ensure_ascii=False, indent=2) + "\n")
    print(f"{'checked' if check else 'recorded'} {len(recorded['cases'])} Markdown hard-row sources")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--evidence", type=Path,
                        default=ROOT / "target/mandoc-migration/snapshot-recording/markdown-rows")
    args = parser.parse_args()
    record(args.check, args.evidence)


if __name__ == "__main__":
    main()
