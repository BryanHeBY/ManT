"""Exact numbered-source recovery policies, separate from syntax acceptance.

Readable expectations follow mant-roff.md's existing N source-spelling
contract. They are computed from canonical authored syntax, never candidate
output. Closed valid indices are deliberately outside this policy.
"""

import hashlib
import json
from pathlib import Path

POLICIES = (Path(__file__).resolve().parents[3]
            / 'crates/mant-codec/src/mandoc/roff_escape/fixtures/escape_policies.json')


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def readable_recovery_rows(case):
    metadata = case['metadata']
    if (case['rule_id'] != 'E03' or metadata.get('outer') != 'N'
            or metadata.get('completion') not in {'opening', 'unclosed'}
            or case['carrier'] not in {'No', 'Lk'}):
        return None
    delimiter = metadata['delimiter']
    if delimiter not in {"'", '|', '\\(aq', '\\[aq]', '\\m[red]', '\\fB'}:
        return None
    if metadata['completion'] == 'unclosed':
        # The consumed malformed numbered escape stays visibly spelled.
        spelling = '\\N' + delimiter + '65'
    elif delimiter in {"'", '|'}:
        # iend leaves the opening literal available after the N prefix.
        spelling = '\\N' + delimiter
    elif delimiter in {'\\(aq', '\\[aq]'}:
        # Its trigger remains available; the normal catalog decodes aq.
        spelling = "\\N'"
    else:
        # Complete m/f openings are re-decoded as nonprinting state.
        spelling = '\\N'
    label_suffix = ': https://example.org' if case['carrier'] == 'Lk' else ''
    return ['A' + spelling + label_suffix + ' AFTER']


def record_policies(cases, identity, reference_sha256, raw_oracles):
    policies = []
    for case in cases:
        rows = readable_recovery_rows(case)
        if rows is None:
            continue
        raw = raw_oracles[case['id']]
        for profile in ['utf8', 'tree']:
            if digest(raw[profile]['stdout']) != case['profiles'][profile]['stdout_sha256']:
                raise ValueError(f'raw reference does not match frozen profile: {case["id"]}/{profile}')
        policies.append(dict(
            id=case['id'], policy_id='numbered-source-recovery-' + case['id'],
            rule='numbered-source-spelling-recovery',
            contract='docs/manuals/mant-roff.md:Escapes (N numbered glyph)',
            upstream='roff_escape.c::roff_escape_impl() iend/unclosed recovery; term.c::term_word() ERROR',
            source_sha256=case['source_sha256'], oracle_identity=identity,
            oracle_sha256=reference_sha256,
            native_utf8_sha256=case['profiles']['utf8']['stdout_sha256'],
            native_tree_sha256=case['profiles']['tree']['stdout_sha256'],
            source_class=case['source_class'],
            match=dict(rule_id=case['rule_id'], carrier=case['carrier'], metadata=case['metadata']),
            native_rows=case['expected_rows'], expected_product_readable_rows=rows,
            reference={profile: raw[profile] for profile in ['utf8', 'tree']},
            expected_axes=['content', 'rows', 'separators'],
            uncovered=['style', 'identity', 'source', 'scalar-range', 'query', 'tui'],
            limits='Only this exact source/id and opening or unclosed numbered argument; no closed valid N, no family exemption',
        ))
    return dict(format_version=1, count=len(policies), policies=policies)


def qualified_policy(case, oracle, binding, policy):
    """Return an exact policy or a binding error; never guess a family match."""
    checks = {
        'id': case['id'], 'source_sha256': digest(case['source']),
        'oracle_identity': binding['identity'],
        'oracle_sha256': binding['reference_sha256'],
        'native_utf8_sha256': digest(oracle['utf8']['stdout']),
        'native_tree_sha256': digest(oracle['tree']['stdout']),
        'match': dict(rule_id=case['rule_id'], carrier=case['carrier'], metadata=case['metadata']),
        'expected_product_readable_rows': readable_recovery_rows(case),
    }
    mismatches = [key for key, value in checks.items() if policy.get(key) != value]
    if readable_recovery_rows(case) is None:
        mismatches.append('non-recovery completion')
    if mismatches:
        return None, 'escape policy binding changed: ' + ', '.join(mismatches)
    return policy, None


def load_policies():
    return {policy['id']: policy for policy in json.loads(POLICIES.read_text())['policies']}
