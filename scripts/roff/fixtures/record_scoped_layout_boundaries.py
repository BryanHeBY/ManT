"""Record exact scoped list, column and display layout from pristine mandoc."""

import argparse
import hashlib
import json
from pathlib import Path
import re

from scripts.roff.fixtures.reference_recipes import PROFILES, arguments, environment
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-codec/src/mandoc/tests/control_boundaries/scoped_layout/cases.json'
FOOTER_DATE = 'November 6, 2026'
FOOTER_TITLE = 'T(1)'


def digest(value):
    return hashlib.sha256(value).hexdigest()


def reading_rows(output):
    """Remove native page furniture once, retaining exact body rows/cells.

    These sixteen sources share a frozen NAME/DESCRIPTION wrapper, contain no
    authored terminal controls and have no requested trailing vertical space.
    print_mdoc_foot() starts at offset zero after one term_vspace() endline;
    its OS/date/title occupy one or two rows on the pinned 78-column device.
    Remove that one proved footer separator, never all trailing empty rows.
    """
    text = output.decode()
    while '\b' in text:
        projected = re.sub(r'.\x08', '', text)
        if projected == text:
            raise ValueError('unpaired native backspace')
        text = projected
    rows = text.split('DESCRIPTION\n', 1)[1].splitlines()
    footer = next((index for index, row in enumerate(rows)
                   if row and not row.startswith('     ')), None)
    if footer is None:
        raise ValueError('missing native footer at the page origin')
    footer_rows = rows[footer:]
    if (len(footer_rows) not in (1, 2)
            or FOOTER_DATE not in footer_rows[-1]
            or not footer_rows[-1].endswith(FOOTER_TITLE)
            or sum(FOOTER_DATE in row for row in footer_rows) != 1):
        raise ValueError('unexpected native footer wrapper')
    body = rows[:footer]
    if not body or body[-1] != '':
        raise ValueError('missing print_mdoc_foot vertical separator')
    body.pop()
    # term_ascii.c defindent is five columns. Do not infer an origin from
    # content, trim authored cells or fold any internal/remaining empty row.
    if any(row and not row.startswith('     ') for row in body):
        raise ValueError('unexpected common native section origin')
    return [row[5:] if row else row for row in body]


def record(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registry = verified_reference(ROOT, reference)
    source_cases = json.loads(FIXTURE.read_text())['cases']
    cases = []
    for source_case in source_cases:
        source = source_case['source'].encode()
        case = {key: source_case[key] for key in ('id', 'source')}
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
            raise SystemExit('scoped-layout pristine receipts changed')
    else:
        FIXTURE.write_text(encoded)
    print(f"verified {result['case_count']} sources / {result['profile_count']} pristine profiles")


if __name__ == '__main__':
    main()
