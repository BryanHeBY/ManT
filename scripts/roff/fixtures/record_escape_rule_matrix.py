"""Freeze escape-rule sources against the registered pristine reference.

Expected output never comes from a product build. Full raw profile evidence
stays in the selected audit directory; the permanent fixture stores exact
sources, observed rows, profile statuses and hashes, and coverage metadata.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path

from scripts.roff.fixtures.escape_rule_cases import cases
from scripts.roff.fixtures.generate_roff_compatibility_fixtures import record_profiles
from scripts.roff.fixtures.acceptance_regions import select_native_region
from scripts.roff.fixtures.roff_fixture_reference import verified_reference
from scripts.roff.fixtures.escape_projection_policies import POLICIES, readable_recovery_rows, record_policies

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'crates/mant-codec/src/mandoc/roff_escape/fixtures/escape_rules.json'


def digest(value):
    return hashlib.sha256(value).hexdigest()


def native_after_styles(raw):
    """Observe backspace overlays from the pristine device, not roff input."""
    cells = []
    cursor = 0
    for character in raw:
        if character == '\b':
            cursor = max(0, cursor - 1)
            continue
        if cursor == len(cells):
            cells.append([character, 0])
        else:
            old, style = cells[cursor]
            if old == character and character not in (' ', '\n'):
                style |= 1
            elif old == '_' or character == '_':
                style |= 2
            cells[cursor] = [old if character == '_' and old != '_' else character, style]
        cursor += 1
    text = ''.join(cell[0] for cell in cells)
    if text.count('AFTER') != 1:
        raise ValueError('font style witness is not unique')
    start = text.index('AFTER')
    return [cell[1] for cell in cells[start:start + 5]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--workers', type=int, default=8)
    args = parser.parse_args()
    registry = verified_reference(ROOT, ROOT / 'target/mandoc-migration/reference/mandoc')
    args.evidence.mkdir(parents=True, exist_ok=True)

    def record(case):
        profiles = record_profiles('escape_rule_grammar', case['id'], case['source'])
        source_hash = digest(case['source'].encode())
        (args.evidence / (case['id'] + '.json')).write_text(
            json.dumps(dict(case, profiles=profiles, source_sha256=source_hash),
                       ensure_ascii=False, indent=2) + '\n')
        region = select_native_region(profiles['utf8']['stdout'], profiles['tree']['stdout'])
        style = {}
        if case['metadata'].get('branch') == 'font-postclass':
            observed = native_after_styles(profiles['utf8']['stdout'])
            style = dict(expected_after_styles=observed)
            if case['metadata'].get('font_extension'):
                # Existing patch 0013/manual contract, independent of ManT:
                # C/V are regular code, VB bold code, VI emphasized code.
                # B/I macro exit selects Roman regardless of operand style.
                mask = {'C': 0, 'V': 0, '[VB]': 1, '[VI]': 2}[case['metadata']['operand']]
                if case['carrier'] != 'TEXT':
                    mask = 0
                style['style_policy'] = dict(
                    rule='documented-pandoc-verbatim-font', source_sha256=source_hash,
                    oracle_identity=registry['identity'],
                    native_utf8_sha256=profiles['utf8']['stdout_sha256'],
                    expected_product_after_styles=[mask] * 5)
        return dict(case, **style, source_sha256=source_hash,
                    expected_rows=region.get('rows') if region['status'] == 'asserted' else None,
                    region_status=region['status'],
                    source_class=('legal' if profiles['lint']['status'] == 0 else 'native-diagnostics'),
                    profiles={name: {key: value for key, value in profile.items()
                                     if key not in ('stdout', 'stderr')}
                              for name, profile in profiles.items()})

    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        rows = list(pool.map(record, cases()))
    bundle = dict(format_version=1, oracle_identity=registry['identity'],
                  count=len(rows), unique_sources=len({row['source_sha256'] for row in rows}),
                  rules={rule: sum(row['rule_id'] == rule for row in rows)
                         for rule in sorted({row['rule_id'] for row in rows})}, cases=rows)
    serialized = json.dumps(bundle, ensure_ascii=False, indent=2) + '\n'
    policies = json.dumps(record_policies(
        rows, registry['identity'],
        hashlib.sha256((ROOT / 'target/mandoc-migration/reference/mandoc').read_bytes()).hexdigest(),
        {case['id']: json.loads((args.evidence / (case['id'] + '.json')).read_text())['profiles']
         for case in rows if readable_recovery_rows(case) is not None}),
        ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if FIXTURE.read_text() != serialized:
            raise SystemExit('escape-rule oracle fixture differs; inspect reference identity and exact sources')
        if POLICIES.read_text() != policies:
            raise SystemExit('escape-rule source recovery policy binding differs')
    else:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(serialized)
        POLICIES.write_text(policies)
    print(json.dumps({key: value for key, value in bundle.items() if key != 'cases'}))


if __name__ == '__main__':
    main()
