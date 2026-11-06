"""Record exact plain/repeated margin-flush word and cell boundaries."""

import argparse
import hashlib
import json
from pathlib import Path
import re

from scripts.roff.fixtures.reference_recipes import PROFILES, arguments, environment
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-codec/src/mandoc/tests/control_boundaries/margin_flush/cases.json'


def digest(value):
    return hashlib.sha256(value).hexdigest()


def reading_rows(output):
    text = output.decode()
    while '\b' in text:
        projected = re.sub(r'.\x08', '', text)
        if projected == text:
            raise ValueError('unpaired native backspace')
        text = projected
    body = text.split('DESCRIPTION\n', 1)[1].split('NEXT\n', 1)[0]
    rows = body.strip('\n').split('\n')
    # term_ascii.c's five-column common section origin is not author text.
    # Keep every remaining SP/NBSP and internal empty row; never trim words.
    if any(row and not row.startswith('     ') for row in rows):
        raise ValueError('unexpected common native section origin')
    return [row[5:] if row else row for row in rows]


def record(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registry = verified_reference(ROOT, reference)
    source_cases = json.loads(FIXTURE.read_text())['cases']
    cases = []
    for source_case in source_cases:
        source = source_case['source'].encode()
        case = {key: source_case[key] for key in ('id', 'source')}
        if 'historical_id' in source_case:
            case['historical_id'] = source_case['historical_id']
        case.update(source_sha256=digest(source), profiles={})
        folder = evidence / case['id']
        folder.mkdir(parents=True, exist_ok=True)
        (folder / 'source.roff').write_bytes(source)
        for profile in PROFILES:
            result = run_reference(reference, arguments(profile), input_bytes=source,
                                   env=environment())
            (folder / (profile + '.stdout')).write_bytes(result.stdout)
            (folder / (profile + '.stderr')).write_bytes(result.stderr)
            if result.returncode != 0 or result.stderr:
                raise ValueError(f"{case['id']}/{profile}: {result.returncode} {result.stderr!r}")
            case['profiles'][profile] = dict(status=result.returncode,
                stdout_sha256=digest(result.stdout), stderr_sha256=digest(result.stderr))
            if profile in ('ascii', 'utf8'):
                case[profile + '_rows'] = reading_rows(result.stdout)
        cases.append(case)
    return dict(oracle_identity=registry['identity'],
                oracle_sha256=digest(reference.read_bytes()),
                case_count=len(cases), profile_count=len(cases) * len(PROFILES), cases=cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--check', action='store_true')
    options = parser.parse_args()
    result = record(options.evidence)
    encoded = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if options.check:
        if FIXTURE.read_text() != encoded:
            raise SystemExit('margin-flush pristine receipts changed')
    else:
        FIXTURE.write_text(encoded)
    print(f"verified {result['case_count']} sources / {result['profile_count']} pristine profiles")


if __name__ == '__main__':
    main()
