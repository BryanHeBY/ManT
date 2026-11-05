"""Authenticate native enclosure operands without executing a ManT candidate."""

import argparse
import hashlib
import json
from pathlib import Path

from scripts.roff.fixtures.enclosure_operand_cases import cases
from scripts.roff.fixtures.generate_roff_compatibility_fixtures import cells, record_profiles
from scripts.roff.fixtures.roff_fixture_reference import verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-engine/tests/roff_lowering/enclosure_operands/cases.json'


def rows_and_fonts(profiles):
    native = [cells(row) for row in profiles['utf8']['stdout'].splitlines()]
    text = [''.join(character for character, _ in row) for row in native]
    start, end = text.index('DESCRIPTION') + 1, text.index('NEXT')
    selected = native[start:end]
    if selected[-1]:
        raise ValueError('missing fixed NEXT separator')
    selected.pop()
    origin_row = next(row for row in selected if ''.join(c for c, _ in row).lstrip(' ') == 'ORIGIN')
    origin = len(origin_row) - len('ORIGIN')
    rows = [''.join(character for character, _ in row[origin:]).rstrip(' ') for row in selected]
    return rows, [[font for _, font in row[origin:origin + len(text)]]
                  for row, text in zip(selected, rows, strict=True)]


def record(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registry = verified_reference(ROOT, reference)
    recorded = []
    for case in cases():
        profiles = record_profiles('enclosure-operands', case['id'], case['source'])
        raw = dict(**case, profiles=profiles)
        if evidence:
            evidence.mkdir(parents=True, exist_ok=True)
            (evidence / (case['id'] + '.json')).write_text(json.dumps(raw, ensure_ascii=False, indent=2) + '\n')
        rows, styles = rows_and_fonts(profiles)
        receipts = {key: dict(status=profile['status'], stdout_sha256=profile['stdout_sha256'],
                              stderr_sha256=profile['stderr_sha256'],
                              diagnostics=profile['stdout'] if key == 'lint' else profile['stderr'])
                    for key, profile in profiles.items()}
        recorded.append(dict(**case, source_sha256=hashlib.sha256(case['source'].encode()).hexdigest(),
                             rows=rows, styles=styles, profiles=receipts))
    return dict(oracle_identity=registry['identity'],
                oracle_sha256=hashlib.sha256(reference.read_bytes()).hexdigest(),
                generated_count=len(recorded), unique_source_count=len({case['source'] for case in recorded}),
                profile_count=5 * len(recorded), cases=recorded)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--write', action='store_true')
    options = parser.parse_args()
    result = record(options.evidence)
    encoded = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if options.write:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(encoded)
    elif FIXTURE.read_text() != encoded:
        raise ValueError('native enclosure receipts changed')
    print(result['generated_count'], 'cases;', result['unique_source_count'], 'unique sources;',
          result['profile_count'], 'native profile receipts')


if __name__ == '__main__':
    main()
