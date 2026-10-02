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

ROOT = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registration = verified_reference(ROOT, reference)
    binding = json.loads((args.cache / 'manifest.json').read_text())
    actual_sha = hashlib.sha256(reference.read_bytes()).hexdigest()
    if (binding['identity'] != registration['identity']
            or binding['reference_sha256'] != actual_sha
            or binding.get('expectations_from_product') is not False):
        raise SystemExit('field oracle cache is not the active pristine identity')
    cache = {row['source_sha256']: row for row in
             map(json.loads, (args.cache / 'cache.jsonl').read_text().splitlines())}
    rows = []
    for case in cases():
        source_hash = hashlib.sha256(case['source'].encode()).hexdigest()
        oracle = cache[source_hash]
        for profile in ('ascii', 'utf8', 'html', 'tree', 'lint'):
            for stream in ('stdout', 'stderr'):
                if hashlib.sha256(oracle[profile][stream].encode()).hexdigest() != oracle[profile][stream + '_sha256']:
                    raise SystemExit('field oracle profile hash changed: ' + case['id'])
        card, region = build_policy(case, oracle, binding)
        qualified, _ = project_regions(region, region, dict(card, product_table_seams=[]))
        witness = owner_witness(case, oracle['tree']['stdout'])
        lint = oracle['lint']['code']
        source_class = ('invalid-generator' if any(oracle[key]['code'] > 2 for key in ('utf8', 'tree'))
                        else 'legal' if lint == 0 else 'diagnosed' if lint <= 2 else 'recovery')
        if not witness['reachable']:
            source_class = 'unreachable'
        rows.append(dict(case, source_sha256=source_hash,
                         oracle_class=source_class, native_owner=witness,
                         native_region=region, expected_rows=qualified.get('rows'),
                         projection=card,
                         profiles={name: {key: value for key, value in profile.items()
                                          if key not in ('stdout', 'stderr')}
                                   for name, profile in oracle.items()
                                   if name != 'source_sha256'}))
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
