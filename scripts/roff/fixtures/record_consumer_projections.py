"""Record consumer-selected native rows from immutable complete roff sources.

Source identities live in the existing consumer fixture. Only independent
pristine profiles can update its native expectations. Product output is never
read; full native bytes remain beneath target, including recovery diagnostics.
"""

import argparse
import hashlib
import json
from pathlib import Path

from scripts.roff.fixtures import acceptance_regions, roff_fixture_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/acceptance_axes/consumer_cases.json"
PROFILES = (
    ("utf8", ("-Tutf8", "-Owidth=78")),
    ("ascii", ("-Tascii", "-Owidth=78")),
    ("html", ("-Thtml",)),
    ("tree", ("-Ttree",)),
    ("lint", ("-Tlint",)),
)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def record(check, evidence):
    binary = ROOT / "target/mandoc-migration/reference/mandoc"
    registration = roff_fixture_reference.verified_reference(ROOT, binary)
    original = json.loads(FIXTURE.read_text())
    expected = {"oracle": {"identity": registration["identity"],
                           "sha256": sha(binary.read_bytes())}, "cases": []}
    raw = {}
    for case in original["cases"]:
        source = case["source"].encode()
        if sha(source) != case["source_sha256"]:
            raise ValueError(f"{case['name']}: immutable source identity changed")
        profiles = {}
        for name, arguments in PROFILES:
            result = roff_fixture_reference.run_reference(
                binary, arguments, input_bytes=source, check=False)
            if name != "lint" and result.returncode:
                raise ValueError(f"{case['name']}: {name} execution failed")
            profiles[name] = {"status": result.returncode,
                              "stdout": result.stdout.decode("utf-8"),
                              "stderr": result.stderr.decode("utf-8"),
                              "stdout_sha256": sha(result.stdout),
                              "stderr_sha256": sha(result.stderr)}
        region = acceptance_regions.select_native_region(
            profiles["utf8"]["stdout"], profiles["tree"]["stdout"])
        if region["status"] != "asserted":
            raise ValueError(f"{case['name']}: native region is unqualified: {region}")
        card = {"name": case["name"], "mechanism": case["mechanism"],
                "source": case["source"], "source_sha256": sha(source),
                "rows": [" ".join(cell for cell in row.split(" ") if cell)
                         for row in region["rows"]],
                "native_rows": region["rows"], "heading": region["following"],
                "admission": profiles["lint"]["status"],
                "profiles": {name: {key: value for key, value in profile.items()
                                    if key not in ("stdout", "stderr")}
                             for name, profile in profiles.items()}}
        expected["cases"].append(card)
        raw[case["name"]] = profiles
    evidence = evidence.resolve()
    if not evidence.is_relative_to(ROOT / "target"):
        raise ValueError("native recording evidence must remain beneath target")
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / "native-profiles.json").write_text(
        json.dumps(raw, ensure_ascii=False, indent=2) + "\n")
    if check:
        if expected != original:
            raise ValueError("consumer fixture differs from the exact pristine runs")
    else:
        FIXTURE.write_text(json.dumps(expected, ensure_ascii=False, indent=2) + "\n")
    print(f"{'checked' if check else 'recorded'} {len(expected['cases'])} consumer sources")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--evidence", type=Path,
                        default=ROOT / "target/mandoc-migration/snapshot-recording/consumer-projections")
    args = parser.parse_args()
    record(args.check, args.evidence)


if __name__ == "__main__":
    main()
