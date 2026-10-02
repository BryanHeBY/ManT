"""Freeze controlled-omission cases from the active five-profile pristine cache.

The recorder reads no product output. Native lint diagnostics remain recorded;
only actual guard inputs determine the independent coverage expectation.
"""

import argparse
import json
from pathlib import Path

from scripts.roff.fixtures import acceptance_regions, integrity_rule_cases
from scripts.roff.fixtures import replay_rule_boundaries as replay
from scripts.roff.fixtures.roff_fixture_reference import verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-engine/tests/escape_coverage/cases.json'


def record(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registration = verified_reference(ROOT, reference)
    binding = {'identity': registration['identity'], 'reference_sha256': replay.sha(reference.read_bytes())}
    cases = replay.assemble(['integrity'])
    _, _, cache = replay.validated_cache(evidence, binding, cases)
    result = []
    for case in cases:
        oracle = cache[case['source_sha256']]
        result.append({
            'id': case['id'], 'source': case['source'], 'source_sha256': case['source_sha256'],
            'metadata': case['metadata'], 'oracle_identity': binding['identity'],
            'oracle_sha256': binding['reference_sha256'],
            'profile_hashes': {name: {key: oracle[name][key] for key in
                                      ('code', 'stdout_sha256', 'stderr_sha256')}
                               for name in replay.PROFILES},
            'native_region': acceptance_regions.select_native_region(
                oracle['utf8']['stdout'], oracle['tree']['stdout']),
        })
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--oracle-evidence', required=True, type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = record(args.oracle_evidence)
    if args.check:
        if json.loads(FIXTURE.read_text()) != result:
            raise SystemExit('coverage fixture changed; inspect exact pristine source/profile binding')
    else:
        FIXTURE.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(f'{len(result)} coverage cases, {sum(row["metadata"]["native_guard_expected"] for row in result)} protected sources')


if __name__ == '__main__':
    main()
