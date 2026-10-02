"""Freeze all field-rule identities from a verified five-profile oracle cache.

The cache must have been collected before product assertions and carry the
active pristine identity. This script never executes or reads a product.
"""

import argparse
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path

from scripts.roff.fixtures.field_projection_policies import (
    FIXTURE, build_policy, owner_witness, project_regions,
)
from scripts.roff.fixtures.roff_fixture_reference import verified_reference
from scripts.roff.fixtures.rule_closure_fields import cases
from scripts.roff.fixtures import replay_rule_boundaries as replay

ROOT = Path(__file__).resolve().parents[3]
# Preserve the existing compressed gold's profile order independently of the
# cache collector's execution order and additional invocation metadata.
FROZEN_PROFILES = ('ascii', 'utf8', 'html', 'lint', 'tree')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registration = verified_reference(ROOT, reference)
    actual_sha = hashlib.sha256(reference.read_bytes()).hexdigest()
    selected = list(cases())
    observers = [dict(case, cohort='field', source_sha256=hashlib.sha256(case['source'].encode()).hexdigest())
                 for case in selected]
    directory, _, cache = replay.validated_cache(
        args.cache.parent, dict(identity=registration['identity'], reference_sha256=actual_sha), observers)
    if directory.resolve() != args.cache.resolve():
        raise SystemExit('field cache directory does not match the active pristine identity')
    binding = json.loads((directory / 'manifest.json').read_text())
    rows = []
    for case, observer in zip(selected, observers):
        source_hash = hashlib.sha256(case['source'].encode()).hexdigest()
        oracle = replay.transport.oracle_record(cache, observer)
        card, region = build_policy(case, oracle, binding)
        qualified, _ = project_regions(region, region, dict(card, product_table_seams=[]))
        witness = owner_witness(case, oracle['tree']['stdout'])
        lint = oracle['lint']['code']
        source_class = ('invalid-generator' if replay.admission(oracle) == 'generator-invalid'
                        else 'legal' if lint == 0 else 'diagnosed' if lint <= 2 else 'recovery')
        if not witness['reachable']:
            source_class = 'unreachable'
        rows.append(dict(case, source_sha256=source_hash,
                         oracle_class=source_class, native_owner=witness,
                         native_region=region, expected_rows=qualified.get('rows'),
                         projection=card,
                         profiles={name: {key: oracle[name][key] for key in
                                          ('code', 'stdout_sha256', 'stderr_sha256')}
                                   for name in FROZEN_PROFILES}))
    header = {'format_version': 1, 'oracle_identity': binding['identity'],
              'oracle_sha256': binding['reference_sha256'], 'expectations_from_product': False,
              'count': len(rows), 'unique_sources': len({row['source_sha256'] for row in rows}),
              'families': dict(sorted(Counter(row['family'] for row in rows).items())),
              'source_classes': dict(sorted(Counter(row['oracle_class'] for row in rows).items())),
              'region_statuses': dict(sorted(Counter(row['native_region']['status'] for row in rows).items()))}
    serialized = '\n'.join(json.dumps(row, ensure_ascii=False, separators=(',', ':'))
                           for row in (header, *rows)) + '\n'
    content = gzip.compress(serialized.encode(), mtime=0)
    if args.check:
        if FIXTURE.read_bytes() != content:
            raise SystemExit('field-rule gold differs; inspect pristine identity and exact source/profile bindings')
    else:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_bytes(content)
    print(json.dumps(header, sort_keys=True))


if __name__ == '__main__':
    main()
