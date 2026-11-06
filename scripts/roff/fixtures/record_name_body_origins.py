"""Bind name-body continuation and leading-row controls to pristine profiles."""

import argparse
import json
from pathlib import Path

from scripts.roff.fixtures.name_field_session_cases import document
from scripts.roff.fixtures.record_name_field_sessions import digest, projected_rows
from scripts.roff.fixtures.reference_recipes import arguments, environment
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-engine/tests/roff_lowering/name_body_handoffs/origins/cases.json'
PROFILES = [('ascii40', 'ascii', 40), ('ascii78', 'ascii', 78),
            ('utf840', 'utf8', 40), ('utf878', 'utf8', 78),
            ('html', 'html', 78), ('tree', 'tree', 78), ('lint', 'lint', 78)]


def sources():
    return [dict(id='one-leading-row', source=document('.Nm probe\n.Ft int\n leading')),
            dict(id='two-leading-rows', source=document('.Nm probe\n.Ft int\n leading\n second'))]


def record(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registry = verified_reference(ROOT, reference)
    cases = []
    for source in sources():
        path = evidence / source['id']
        path.mkdir(parents=True, exist_ok=True)
        (path / 'source.roff').write_text(source['source'])
        case = dict(**source, source_sha256=digest(source['source']), profiles={},
                    native_rows=[], native_styles=[])
        for name, profile, width in PROFILES:
            run = run_reference(reference, arguments(profile, width=width),
                                input_bytes=source['source'].encode(), env=environment())
            if run.returncode != 0 or run.stderr:
                raise ValueError(f"{source['id']}/{name}: {run.returncode} {run.stderr!r}")
            (path / (name + '.stdout')).write_bytes(run.stdout)
            (path / (name + '.stderr')).write_bytes(run.stderr)
            case['profiles'][name] = dict(status=run.returncode,
                stdout_sha256=digest(run.stdout), stderr_sha256=digest(run.stderr))
            if profile in ('ascii', 'utf8'):
                rows, styles = projected_rows(run.stdout)
                if case['native_rows'] and rows != case['native_rows']:
                    raise ValueError(f"{source['id']}/{name}: changed physical row origins")
                case['native_rows'], case['native_styles'] = rows, styles
        cases.append(case)
    return dict(oracle_identity=registry['identity'], oracle_sha256=digest(reference.read_bytes()),
                case_count=len(cases), profile_count=len(cases) * len(PROFILES), cases=cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = record(args.evidence)
    encoded = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if FIXTURE.read_text() != encoded:
            raise SystemExit('name-body origin pristine receipts changed')
    else:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(encoded)
    print(f"verified {result['case_count']} name-body origins / {result['profile_count']} profiles")


if __name__ == '__main__':
    main()
