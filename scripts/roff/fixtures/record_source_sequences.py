#!/usr/bin/env python3
"""Record bounded source-sequence contracts from the pristine mandoc oracle.

Each table retains its own five profiles and its independently valid fragment's
five profiles. Unsupported whole-table syntax is never called a native-supported
case. --check compares complete receipts without accepting a product candidate.
"""

import argparse
import hashlib
import json
from pathlib import Path

from scripts.roff.fixtures import source_sequence_cases
from scripts.roff.fixtures.generate_roff_compatibility_fixtures import cells, record_profiles
from scripts.roff.fixtures.roff_fixture_reference import verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-engine/tests/roff_lowering/source_sequences/cases.json'


def digest(value):
    return hashlib.sha256(value).hexdigest()


def row_cells(profiles, heading):
    """Retain exact interior/empty rows; remove only the shared manual origin."""
    native = [cells(line) for line in profiles['utf8']['stdout'].splitlines()]
    decoded = [''.join(character for character, _ in row) for row in native]
    start = next(index for index, row in enumerate(decoded) if row == heading) + 1
    end = next(index for index in range(start, len(decoded)) if decoded[index] == 'NEXT')
    if decoded[end - 1] != '':
        raise ValueError('oracle NEXT separator missing')
    selected = decoded[start:end - 1]
    origin = min((len(row) - len(row.lstrip(' ')) for row in selected if row), default=0)
    result = []
    for index, row in enumerate(selected, start):
        content = row[origin:].rstrip(' ') if row else ''
        result.append((content, [font for _, font in native[index][origin:origin + len(content)]]))
    return result


def rows(profiles, heading):
    return [row for row, _ in row_cells(profiles, heading)]


def run(case, source, evidence, role):
    profiles = record_profiles('source-sequence', case['id'] + '-' + role, source)
    path = evidence / (case['id'] + '-' + role + '.json')
    path.write_text(json.dumps(dict(source=source, profiles=profiles), ensure_ascii=False,
                               indent=2) + '\n')
    receipts = {profile: dict(status=value['status'],
                             stdout_sha256=value['stdout_sha256'],
                             stderr_sha256=value['stderr_sha256'],
                             diagnostics=value['stdout'] if profile == 'lint' else value['stderr'])
                for profile, value in profiles.items()}
    content = row_cells(profiles, case['section'])
    return dict(source_sha256=digest(source.encode()), rows=[row for row, _ in content],
                styles=[fonts for _, fonts in content], profiles=receipts)


def record(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registry = verified_reference(ROOT, reference)
    evidence.mkdir(parents=True, exist_ok=True)
    recorded = []
    for case in source_sequence_cases.cases():
        actual = run(case, case['source'], evidence, 'whole')
        independent = run(case, case['independent_source'], evidence, 'fragment')
        code_text = None
        if case['cohort'] == 'core' and case['id'].endswith('-In'):
            # Existing append_include compacts its executed output boundary,
            # including its incoming separator, into one semantic Code owner.
            # The actual native row independently proves the accepted spelling
            # after zero-advance/continuation, rather than the authored `<`.
            if case['section'] == 'SYNOPSIS':
                code_text = independent['rows'][-1]
            else:
                if not independent['rows'][0].startswith('arg'):
                    raise ValueError('include witness lost its preceding operand')
                code_text = independent['rows'][0][len('arg'):]
        recorded.append(dict(**case, code_text=code_text, whole=actual, independent=independent))
    return dict(oracle_identity=registry['identity'], oracle_sha256=digest(reference.read_bytes()),
                core_count=60, risk_count=16, bibliography_clean_count=6,
                bibliography_warning_count=8, bibliography_root_count=6, cases=recorded,
                source_entries=[dict(**case, native=run(case, case['source'], evidence, 'entry'))
                                for case in source_sequence_cases.source_entries()])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = record(args.evidence)
    encoded = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if FIXTURE.read_text() != encoded:
            raise SystemExit('pristine source-sequence receipts changed')
    else:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(encoded)
    print(f'{len(result["cases"])} dual-source cases and '
          f'{len(result["source_entries"])} source-entry controls, all five-profile witnessed')


if __name__ == '__main__':
    main()
