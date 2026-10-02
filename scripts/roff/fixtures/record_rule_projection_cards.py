"""Bind generated device decoration edits to exact retained sources and oracle.

This records source/AST-proven inset separator and margin cells. No candidate
is read, authored NBSP is never replaced, and hard rows are never collapsed.
"""

import argparse
import gzip
import json
from pathlib import Path

from scripts.roff.fixtures import field_projection_policies as fields
from scripts.roff.fixtures import replay_rule_boundaries as replay
from scripts.roff.fixtures.roff_fixture_reference import verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / 'tests/fixtures/roff/rule_boundaries/projection_cards.json.gz'


def cards(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registration = verified_reference(ROOT, reference)
    binding = {'identity': registration['identity'], 'reference_sha256': replay.sha(reference.read_bytes())}
    cases = [case for case in replay.assemble(['retained-review'])
             if case['family'] in ('field-transition', 'ti-transition', 'legal-list-controls')]
    _, _, cache = replay.validated_cache(evidence, binding, cases)
    result = []
    for case in cases:
        oracle = cache[case['source_sha256']]
        card, region = fields.build_policy(case, oracle, binding)
        add_mixed_blank_cells(case, oracle, card, region)
        if card['rules']:
            card['type'] = 'device-projection'
            card['contract'] = 'docs/manuals/mant-roff.md: mdoc definition generated cells; responsive layout'
            card['scope'] = 'exact source/id and observed native row/scalar cells; no authored-space or hard-row edit'
            result.append(card)
    return result


def add_mixed_blank_cells(case, oracle, card, region):
    """Keep authored fixed blanks; edit only a proven It BODY generated cell."""
    metadata = case['metadata']
    if (case['family'] != 'field-transition' or metadata.get('label') != '\\0'
            or metadata.get('kind') not in ('inset', 'diag')
            or case['source'].count('\\0') != 1 or region.get('status') != 'asserted'):
        return
    nodes = fields.tree_owners(oracle['tree']['stdout'])
    body = [node for node in nodes if node['kind'] == 'text' and node['label'] == 'BodyWord']
    authored = [node for node in nodes if node['kind'] == 'text' and node['label'] == '\\0']
    if len(body) != 1 or body[0]['owner'] != 'It BODY' or len(authored) != 1:
        raise ValueError('mixed fixed-blank source ownership changed: ' + case['id'])
    inset = metadata['kind'] == 'inset'
    if authored[0]['owner'] != ('It HEAD' if inset else 'It BODY'):
        raise ValueError('authored fixed blank changed owner: ' + case['id'])
    needle = 'AFTER\u00a0BodyWord' if inset else 'Xo\u00a0\u00a0'
    positions = [(row, text.index(needle)) for row, text in enumerate(region['rows']) if needle in text]
    if len(positions) != 1:
        raise ValueError('generated BODY cells have no unique witness: ' + case['id'])
    row, start = positions[0]
    start += len('AFTER') if inset else len('Xo')
    card['native_replacements'] = [dict(row=row, column=start + index, **{'from': '\u00a0', 'to': ' '})
                                   for index in range(1 if inset else 2)]
    card['rules'].append('mdoc_term.c:764 LIST_inset/diag generated BODY only; separate authored \\0 preserved')


def integrity_cards(evidence):
    reference = ROOT / 'target/mandoc-migration/reference/mandoc'
    registration = verified_reference(ROOT, reference)
    binding = {'identity': registration['identity'], 'reference_sha256': replay.sha(reference.read_bytes())}
    cases = replay.assemble(['integrity'])
    _, _, cache = replay.validated_cache(evidence, binding, cases)
    result = []
    for case in cases:
        if case['id'] == 'coverage-control-source-spelling-fallback':
            # Existing reading contract preserves unsupported N source spelling;
            # this exact operand has no stateful suffix or pending glyph.
            expected = ['A\\N|65535|Z AFTER']
            rule = 'documented-source-spelling-recovery'
        elif (case['family'] == 'coverage-table-owners'
              and case['metadata']['owner'] == 'source-fragment'
              and case['metadata']['depth'] in (255, 256)):
            # tbl_read receives quoted macro operands. The documented closed
            # T{} recovery executes No's argument quotes, not literal quotes.
            # Pristine five-profile runs retain both original quoted rows.
            expected = ['AZ', 'AFTER']
            rule = 'closed-table-inline-recovery'
        else:
            continue
        oracle = cache[case['source_sha256']]
        result.append(dict(id=case['id'], policy_id='integrity-reading-' + case['id'],
            source_sha256=case['source_sha256'], oracle_identity=binding['identity'],
            oracle_sha256=binding['reference_sha256'],
            native_utf8_sha256=oracle['utf8']['stdout_sha256'],
            native_tree_sha256=oracle['tree']['stdout_sha256'],
            expected_product_rows=expected, rule=rule,
            contract='docs/manuals/mant-roff.md: Escapes and transactional closed Tables recovery',
            uncovered=['style', 'source', 'scalar-range', 'query', 'tui']))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--oracle-evidence', required=True, type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = cards(args.oracle_evidence)
    encoded = json.dumps(result, ensure_ascii=False, separators=(',', ':')).encode() + b'\n'
    if args.check:
        if gzip.decompress(FIXTURE.read_bytes()) != encoded:
            raise SystemExit('rule projection cards changed: inspect exact source/pristine bindings')
    else:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_bytes(gzip.compress(encoded, mtime=0))
    print(f'{len(result)} exact device projection cards')
    integrity = integrity_cards(args.oracle_evidence)
    path = FIXTURE.with_name('integrity_projection_cards.json')
    if args.check:
        if json.loads(path.read_text()) != integrity:
            raise SystemExit('integrity reading cards changed: inspect source/pristine bindings')
    else:
        path.write_text(json.dumps(integrity, ensure_ascii=False, indent=2) + '\n')
    print(f'{len(integrity)} exact existing-reading-contract cards')


if __name__ == '__main__':
    main()
