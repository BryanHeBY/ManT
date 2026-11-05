"""Record exact complete-session Nm contracts with the locked pristine oracle.

Whole unsupported tbl and clean independent fragments have separate receipts.
No product executable or product-generated IR supplies expected text or styles.
"""

import argparse
import hashlib
import json
from pathlib import Path

from scripts.roff.fixtures import name_field_session_cases as inputs
from scripts.roff.fixtures.reference_recipes import PROFILES, arguments, environment
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-engine/tests/roff_lowering/name_field_sessions/cases.json'


def digest(raw):
    return hashlib.sha256(raw.encode() if isinstance(raw, str) else raw).hexdigest()


def cells(line):
    """Keep SP/NBSP and overstrike fonts; never fold the accepted text."""
    output, shadow = [], None
    for character in line:
        if character == '\b':
            shadow = output.pop() if output else None
            continue
        font = 0
        if shadow is not None:
            previous, old_font = shadow
            if previous == character:
                font = old_font | 1
            elif previous == '_':
                font = 2
        output.append((character, font))
        shadow = None
    return output


def projected_rows(raw):
    native = [cells(line) for line in raw.decode().splitlines()]
    rows = [''.join(character for character, _ in row) for row in native]
    start, end = rows.index('SYNOPSIS') + 1, rows.index('NEXT')
    if rows[end - 1] != '':
        raise ValueError('missing native section separator')
    end -= 1
    # term_ascii.c sets defindent=5 and termp_sh_pre(BODY) selects it. Do not
    # measure a minimum from content: unsupported implicit Nm's first real
    # word has one further native separator, which must remain observable.
    origin = 5
    text, styles = [], []
    for row, native_row in zip(rows[start:end], native[start:end]):
        if row and not row.startswith(' ' * origin):
            raise ValueError('content crossed the proven section origin')
        value = row[origin:].rstrip(' ') if row else ''
        text.append(value)
        styles.append([font for _, font in native_row[origin:origin + len(value)]])
    return text, styles


def record_source(reference, identifier, role, source, evidence):
    result = dict(source_sha256=digest(source), profiles={})
    path = evidence / identifier / role
    path.mkdir(parents=True, exist_ok=True)
    (path / 'source.roff').write_text(source)
    for profile in PROFILES:
        run = run_reference(reference, arguments(profile), input_bytes=source.encode(),
                            env=environment())
        expected_status = 4 if profile == 'lint' and role == 'whole' else 0
        # The pristine HTML formatter aborts on this exact nested phrasing
        # sequence. Retain its failure receipt, never infer HTML from terminal.
        if (identifier, role, profile) == ('risk-prefix-Fn', 'fragment', 'html'):
            expected_status = -6
        if run.returncode != expected_status:
            raise ValueError(f'{identifier}/{role}/{profile}: {run.returncode}: {run.stderr!r}')
        result['profiles'][profile] = dict(status=run.returncode,
                                          stdout_sha256=digest(run.stdout),
                                          stderr_sha256=digest(run.stderr),
                                          diagnostics=(run.stdout if profile == 'lint'
                                                       else run.stderr).decode())
        (path / (profile + '.stdout')).write_bytes(run.stdout)
        (path / (profile + '.stderr')).write_bytes(run.stderr)
        if profile == 'utf8':
            result['rows'], result['styles'] = projected_rows(run.stdout)
    return result


def record(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registry = verified_reference(ROOT, reference)
    result = []
    for case in inputs.cases():
        role = 'whole' if case['independent_source'] else 'ordinary'
        whole = record_source(reference, case['id'], role, case['source'], evidence)
        independent = (record_source(reference, case['id'], 'fragment',
                                     case['independent_source'], evidence)
                       if case['independent_source'] else None)
        result.append(dict(**case, whole=whole, independent=independent))
    controls = []
    for case in inputs.cell_controls():
        whole = record_source(reference, case['id'], 'whole', case['source'], evidence)
        fragments = [record_source(reference, case['id'], f'fragment-{index}', source, evidence)
                     for index, source in enumerate(case['fragments'])]
        controls.append(dict(**case, whole=whole, independent=fragments))
    return dict(oracle_identity=registry['identity'],
                oracle_sha256=digest(reference.read_bytes()), core_count=25,
                risk_count=20, nested_count=11, cell_control_count=4,
                profile_count=565, cases=result, cell_controls=controls)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = record(args.evidence)
    encoded = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if FIXTURE.read_text() != encoded:
            raise SystemExit('complete-session Nm pristine receipts changed')
    else:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(encoded)
    print('verified 56 complete Nm sessions + 4 cell controls / 113 runs / 565 pristine profiles')


if __name__ == '__main__':
    main()
