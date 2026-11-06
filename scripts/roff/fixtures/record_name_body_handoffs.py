"""Record ordinary It/Nm handoffs with pristine five-profile receipts first."""

import argparse
import json
from pathlib import Path

from scripts.roff.fixtures.name_body_handoff_cases import cases
from scripts.roff.fixtures.record_name_field_sessions import digest, projected_rows
from scripts.roff.fixtures.reference_recipes import PROFILES, arguments, environment
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-engine/tests/roff_lowering/name_body_handoffs/cases.json'


def record(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registry = verified_reference(ROOT, reference)
    output = []
    for case in cases():
        path = evidence / case['id']
        path.mkdir(parents=True, exist_ok=True)
        (path / 'source.roff').write_text(case['source'])
        oracle = dict(source_sha256=digest(case['source']), profiles={})
        for profile in PROFILES:
            run = run_reference(reference, arguments(profile), input_bytes=case['source'].encode(),
                                env=environment())
            expected = 4 if case['context'] == 'table' and profile == 'lint' else 0
            if run.returncode != expected:
                raise ValueError(f"{case['id']}/{profile}: {run.returncode} {run.stdout!r} {run.stderr!r}")
            oracle['profiles'][profile] = dict(status=run.returncode,
                stdout_sha256=digest(run.stdout), stderr_sha256=digest(run.stderr),
                diagnostics=(run.stdout if profile == 'lint' else run.stderr).decode())
            (path / (profile + '.stdout')).write_bytes(run.stdout)
            (path / (profile + '.stderr')).write_bytes(run.stderr)
            if profile == 'utf8':
                oracle['rows'], oracle['styles'] = projected_rows(run.stdout)
        output.append(dict(**case, oracle=oracle))
    return dict(oracle_identity=registry['identity'],
                oracle_sha256=digest(reference.read_bytes()),
                case_count=len(output), profile_count=len(output) * len(PROFILES), cases=output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = record(args.evidence)
    encoded = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if FIXTURE.read_text() != encoded:
            raise SystemExit('ordinary It/Nm pristine receipts changed')
    else:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(encoded)
    print(f"verified {result['case_count']} ordinary It/Nm sessions / {result['profile_count']} profiles")


if __name__ == '__main__':
    main()
