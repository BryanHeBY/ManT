"""Check supplemental field fixtures against all five pristine profiles.

This verifier never writes fixtures or reads a product executable. Pure shape
checks also run in the existing field-fixture gate; raw oracle replay is an
explicit --check operation, with unchanged stdout bytes retained below target.
"""

import argparse
from collections import Counter
import concurrent.futures
import hashlib
import itertools
import json
from pathlib import Path
import re
import unicodedata

from scripts.roff.fixtures import field_projection_policies as fields
from scripts.roff.fixtures.acceptance_regions import select_native_region
from scripts.roff.fixtures.replay_rule_boundaries import admission, validate_oracle_record
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference
from scripts.roff.oracle import mandoc_oracle

ROOT = Path(__file__).resolve().parents[3]
DIRECTORY = ROOT / 'crates/mant-engine/tests/roff_lowering/control_request_matrix'
PROFILES = ('ascii', 'utf8', 'html', 'tree', 'lint')
FIXTURES = {'fixed_blank_rows': (32, 32), 'kept_field_rows': (72, 72),
            'head_word_padding_rows': (189, 187), 'pending_prefix_rows': (540, 540)}
EMPTY_WORD_FIXTURE = ROOT / 'crates/mant-engine/tests/roff_lowering/empty_word_columns/cases.json'
EMPTY_WORD_ATOMS = {'zero-graph': r'\&', 'empty': '', 'combining': r'\[u0301]',
                    'font-only': r'\fB', 'bare-zero-advance': r'\z',
                    'pending-zero-advance': r'\zX', 'author-space': ' ',
                    'fixed-space': r'\0', 'nonbreaking-space': r'\~', 'literal': 'K'}
EMPTY_WORD_CONTROLS = {'spacing-off': '.Sm off', 'glued': '.Ns',
                       'continued-prefix': r'.No "PRE\c"'}
PATTERNS = {
    'pending-accepted': (r'\zX',),
    'pending-accepted-marker': (r'\zX\p',),
    'pending-accepted-repeat-marker': (r'\zX\p\p',),
    'zero-graph-accepted-prefix': (r'\&\p \zX',),
    'accepted-prefix-rejected-pending': (r'X\p \p \zY',),
    'first-rejected-pending': (r'\p \zY',),
    'cross-word-prefix-rejection': (r'X\p \p', r'\zY'),
    'cross-word-first-rejection': (r'\p', r'\zY'),
    'cached-prefix-marker-rejection': (r'\zX\p', r'\p Q'),
}
AXES = {'kind': ['tag', 'hang'], 'width': [0, 2, 4],
        'carrier': ['No', 'Em', 'Sy', 'Li', 'Lk'],
        'entry': ['fresh-head', 'committed-head'],
        'marker_patterns': sorted(PATTERNS)}


def sha(raw):
    return hashlib.sha256(raw.encode() if isinstance(raw, str) else raw).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def valid_hash(value):
    return isinstance(value, str) and re.fullmatch(r'[0-9a-f]{64}', value) is not None


def validate_admission(case, profiles):
    """Use actual lint and usable rendering/AST statuses, never metadata alone."""
    require(isinstance(profiles, dict) and set(profiles) == set(PROFILES)
            and all(isinstance(profile, dict) and type(profile.get('code')) is int
                    and type(profile.get('utf8_valid', True)) is bool
                    and type(profile.get('timeout', False)) is bool for profile in profiles.values()),
            case['id'] + ': invalid admission status format')
    expected = {'valid': 'legal', 'diagnosed-valid': 'diagnosed', 'recovery': 'recovery'}.get(admission(profiles))
    require(expected is not None and case.get('oracle_class') == expected,
            case['id'] + ': oracle admission class contradicts profile statuses')


def registered_binding():
    """Read the committed active attestation without requiring a local binary."""
    registry = json.loads((ROOT / mandoc_oracle.REGISTRY).read_text())
    active = [entry for entry in registry['attestations'].values() if entry['status'] == 'active']
    require(len(active) == 1, 'exactly one active fixture oracle is required')
    path = mandoc_oracle.validate_hash_record(
        ROOT, {key: active[0][key] for key in ('path', 'sha256')}, 'fixture attestation')
    record = json.loads(path.read_text())
    require(record['source']['pristine'] is True, 'fixture oracle must be pristine')
    return {'identity': record['identity'], 'reference_sha256': record['artifact']['sha256']}


def pending_source(case):
    lines = ['.Dd October 2, 2026', '.Dt TEST 1', '.Os', '.Sh NAME', '.Nm test',
             '.Nd probe', '.Sh DESCRIPTION', f".Bl -{case['kind']} -width {case['width']}n"]
    lines += ['.It Xo X', '.br'] if case['entry'] == 'committed-head' else ['.It Xo']
    prefix = '.' + case['carrier'] + (' https://example.org' if case['carrier'] == 'Lk' else '')
    lines += [prefix + ' "' + word + '"' for word in PATTERNS[case['pattern']]]
    return '\n'.join(lines + ['.Xc', '.No BodyWord', '.El', '.Sh NEXT', '.No END', ''])


def validate_position(position, source, label, require_owner=True):
    require(isinstance(position, dict), label + ': position must be an object')
    line, column = position.get('line'), position.get('column')
    rows = source.split('\n')
    require(type(line) is int and type(column) is int and 0 < line <= len(rows)
            and 0 < column <= len(rows[line - 1]) + 1, label + ': source coordinate out of range')
    if require_owner:
        require(position.get('owner') in ('It HEAD', 'It BODY'), label + ': missing source owner')
        require(type(position.get('no_fill')) is bool, label + ': missing no-fill fact')


def validate_fixture(name, fixture, binding=None):
    """Validate finite identities and explicit evidence, never inventing gold."""
    binding = registered_binding() if binding is None else binding
    require(name in FIXTURES and isinstance(fixture, dict), 'unknown supplement fixture')
    header, cases = fixture.get('header'), fixture.get('cases')
    require(isinstance(header, dict) and isinstance(cases, list), 'header/cases shape changed')
    count, unique = FIXTURES[name]
    require(header.get('count') == count and len(cases) == count, 'identity count changed')
    require(header.get('unique_sources') == unique, 'unique source count changed')
    require(header.get('expectations_from_product') is False, 'product expectations are forbidden')
    require(header.get('oracle_sha256') == binding['reference_sha256'], 'oracle hash changed')
    if 'oracle_identity' in header:
        require(header['oracle_identity'] == binding['identity'], 'oracle identity changed')
    require(len({case.get('id') for case in cases}) == count, 'duplicate supplement identity')
    hashes = set()
    for case in cases:
        identity = case.get('id')
        require(isinstance(identity, str) and identity, 'missing supplement identity')
        source = case.get('source')
        require(isinstance(source, str) and source.endswith('\n'), identity + ': missing exact source')
        require(valid_hash(case.get('source_sha256')) and sha(source) == case['source_sha256'],
                identity + ': source hash changed')
        hashes.add(case['source_sha256'])
        require(case.get('oracle_class') in ('legal', 'diagnosed', 'recovery'), identity + ': invalid admission')
        rows = case.get('expected_rows')
        require(isinstance(rows, list) and all(isinstance(row, str) and '\n' not in row for row in rows),
                identity + ': expected physical rows must be explicit')
        profile_hashes = case.get('profile_sha256')
        if profile_hashes is None:
            profiles = case.get('profiles')
            require(isinstance(profiles, dict) and set(profiles) == set(PROFILES), identity + ': missing five profiles')
            for profile in profiles.values():
                require(isinstance(profile, dict) and type(profile.get('code')) is int
                        and profile['code'] in (0, 1, 2) and profile.get('utf8_valid') is True
                        and not profile.get('timeout', False), identity + ': incomplete oracle profile')
                require(valid_hash(profile.get('stdout_sha256')) and valid_hash(profile.get('stderr_sha256')),
                        identity + ': invalid profile stream hash')
            validate_admission(case, profiles)
        else:
            require(isinstance(profile_hashes, dict) and set(profile_hashes) == set(PROFILES)
                    and all(valid_hash(value) for value in profile_hashes.values()), identity + ': invalid five profile hashes')
        owner = case.get('native_owner')
        if owner is not None:
            require(isinstance(owner, dict) and owner.get('reachable') is True
                    and all(isinstance(owner.get(key), list) for key in ('after_owners', 'branch_nodes', 'literal_tabs'))
                    and owner['after_owners'], identity + ': owner unreachable')
            for position in owner['after_owners'] + owner['branch_nodes'] + owner['literal_tabs']:
                validate_position(position, source, identity)
            require(type(owner.get('empty_texts')) is int and owner['empty_texts'] >= 0
                    and type(owner.get('no_text_operand')) is bool, identity + ': owner evidence incomplete')
        if name == 'head_word_padding_rows':
            for key in ('head_word_source', 'body_word_source'):
                validate_position(case.get(key), source, identity, require_owner=False)
            require(isinstance(case.get('expected_head_words'), str), identity + ': missing HEAD word assertion')
            # These exact sources have one independently located BODY operand.
            # Device padding is responsive; accepted HEAD word joins are not.
            require(sum(row.count('BODY') for row in rows) == 1, identity + ': BODY row witness changed')
            head = ' '.join(rows).split('BODY', 1)[0]
            require(' '.join(word for word in head.split(' ') if word) == case['expected_head_words'],
                    identity + ': HEAD word assertion disagrees with explicit rows')
        if name == 'fixed_blank_rows':
            validate_position(case.get('authored_fixed_blank'), source, identity)
        card = case.get('projection', {})
        require(isinstance(card, dict), identity + ': projection must be an object')
        if 'id' in card:
            require(card['id'] == identity and card.get('source_sha256') == case['source_sha256']
                    and card.get('oracle_identity') == binding['identity']
                    and card.get('oracle_sha256') == binding['reference_sha256'], identity + ': projection binding changed')
            require(card.get('native_utf8_sha256') == profile_hashes['utf8']
                    and card.get('native_tree_sha256') == profile_hashes['tree'], identity + ': projection raw hash changed')
        for cell in card.get('native_replacements', []):
            row, column = cell.get('row'), cell.get('column')
            require(type(row) is int and type(column) is int and 0 <= row < len(rows)
                    and 0 <= column < len(rows[row]) and rows[row][column] == cell.get('to')
                    and cell.get('from') == '\u00a0' and cell.get('to') == ' ', identity + ': projection coordinate changed')
    require(len(hashes) == unique, 'unique source count changed')
    if name == 'pending_prefix_rows':
        require(header.get('format_version') == 1 and header.get('axes') == AXES, 'pending axes/format changed')
        require(header.get('source_classes') == dict(Counter(case['oracle_class'] for case in cases)), 'source admission totals changed')
        tuples = {(case.get('kind'), case.get('width'), case.get('carrier'), case.get('entry'), case.get('pattern'))
                  for case in cases}
        require(tuples == set(itertools.product(AXES['kind'], AXES['width'], AXES['carrier'],
                                                AXES['entry'], AXES['marker_patterns'])), 'pending Cartesian coverage changed')
        for case in cases:
            require(case['source'] == pending_source(case), case['id'] + ': pending metadata/source changed')
    return cases


def validate_live_case(name, case, profiles):
    """Check raw hashes first, then actual AST ancestry and exact hard rows."""
    validate_oracle_record(case, dict(source_sha256=case['source_sha256'], **profiles))
    validate_admission(case, profiles)
    for profile in PROFILES:
        raw = profiles[profile]
        # lint reports status 3 for the frozen diagnosed diag/Xc recovery.
        # Rendering and AST profiles must remain usable, never partial gold.
        statuses = (0, 1, 2, 3) if profile == 'lint' else (0, 1, 2)
        require(raw['code'] in statuses and raw['utf8_valid'] and not raw['timeout'], case['id'] + ': oracle failed')
        expected = (case['profile_sha256'][profile] if 'profile_sha256' in case
                    else case['profiles'][profile]['stdout_sha256'])
        require(raw['stdout_sha256'] == expected, case['id'] + ': raw ' + profile + ' hash changed')
        if 'profiles' in case:
            for key in ('code', 'utf8_valid', 'stderr_sha256'):
                require(raw[key] == case['profiles'][profile][key], case['id'] + ': profile status/stderr changed')
    region = select_native_region(profiles['utf8']['stdout'], profiles['tree']['stdout'],
                                  margin_policy=case.get('projection', {}).get('margin_policy'))
    require(region['status'] == 'asserted', case['id'] + ': native region not asserted')
    card = dict(case.get('projection', {}), product_table_seams=[])
    qualified, _ = fields.project_regions(region, region, card)
    require(qualified['rows'] == case['expected_rows'], case['id'] + ': explicit native rows changed')
    nodes = fields.tree_owners(profiles['tree']['stdout'])
    owner = case.get('native_owner', {})
    for position in owner.get('after_owners', []) + owner.get('literal_tabs', []):
        require(any(node['kind'] == 'text' and all(node.get(key) == position[key]
                    for key in ('line', 'column', 'owner', 'no_fill')) for node in nodes), case['id'] + ': actual source owner changed')
    for branch in owner.get('branch_nodes', []):
        require(any(all(node.get(key) == branch[key] for key in ('line', 'column', 'owner', 'no_fill', 'kind', 'label'))
                    for node in nodes), case['id'] + ': actual branch changed')
    for key, expected_owner in (('head_word_source', 'It HEAD'), ('body_word_source', 'It BODY'),
                                 ('authored_fixed_blank', None)):
        if key not in case:
            continue
        position = case[key]
        require(any(node['kind'] == 'text' and node['line'] == position['line'] and node['column'] == position['column']
                    and node['owner'] == (expected_owner or position['owner'])
                    and (key != 'authored_fixed_blank' or node['label'] == r'\0') for node in nodes),
                case['id'] + ': actual ' + key + ' changed')
    return {'id': case['id'], 'source_sha256': case['source_sha256'], 'rows': qualified['rows'], 'status': 'verified'}


def empty_word_source(metadata):
    """Rebuild input axes only, never simulate formatter output."""
    lines = ['.Dd September 28, 2026', '.Dt TEST 1', '.Os', '.Sh NAME',
             '.Nm test', '.Nd test page', '.Sh DESCRIPTION']
    if metadata['mode'] == 'no-fill':
        lines.append('.nf')
    if metadata['start']:
        lines.append('.No START')
    for index in range(metadata['repeat']):
        lines.append('.sp -1')
        if metadata['mode'] == 'entry-control':
            if metadata['preceding_operand'] is not None:
                lines.append(metadata['preceding_operand'])
            lines += [metadata['request'], r'.No \&']
        else:
            lines += ['.No ""', '.br']
            if 'control' in metadata:
                lines.append(EMPTY_WORD_CONTROLS[metadata['control']])
            carrier = '.' + metadata['carrier']
            if metadata['carrier'] == 'Lk':
                carrier += ' ' + metadata.get('active_target', metadata.get('visible_target', 'x'))
            operand = EMPTY_WORD_ATOMS[metadata['atom']]
            if 'active_label' in metadata:
                operand = metadata['active_label'] + (operand if metadata['atom'] != 'literal' else '')
            lines.append(carrier + ' "' + operand + '"')
        lines.append(f'.No BODY_{index}')
    if metadata['mode'] == 'no-fill':
        lines.append('.fi')
    return '\n'.join(lines + ['.Sh NEXT', '.No END', ''])


def empty_word_axes():
    """The separate 472-source cohort leaves the existing 833 unchanged."""
    result = []
    for mode, start, repeat, atom, carrier in itertools.product(
            ('filled', 'no-fill'), (False, True), (1, 2, 3),
            tuple(EMPTY_WORD_ATOMS)[:6], ('No', 'Em', 'Sy', 'Li', 'Lk')):
        result.append(dict(mode=mode, start=start, repeat=repeat, atom=atom, carrier=carrier))
    for preceding, request, start in itertools.product(
            ('.No ""', r'.No \fB', '', None), ('.br', '.sp 0', '.fi', '.nf'), (False, True)):
        result.append(dict(mode='entry-control', start=start, repeat=1, atom='zero-graph',
                           carrier='No', preceding_operand=preceding, request=request))
    for mode, start, repeat, atom in itertools.product(
            ('filled', 'no-fill'), (False, True), (1, 2, 3),
            ('author-space', 'fixed-space', 'nonbreaking-space')):
        result.append(dict(mode=mode, start=start, repeat=repeat, atom=atom, carrier='No'))
    for mode, atom in itertools.product(('filled', 'no-fill'), ('zero-graph', 'pending-zero-advance')):
        result.append(dict(mode=mode, start=True, repeat=1, atom=atom, carrier='Lk',
                           visible_target='https://ex.org', long_row=True))
    for mode, start, control, atom in itertools.product(
            ('filled', 'no-fill'), (False, True), tuple(EMPTY_WORD_CONTROLS),
            ('zero-graph', 'empty', 'pending-zero-advance')):
        result.append(dict(mode=mode, start=start, repeat=1, atom=atom, carrier='No', control=control))
    for mode, atom in itertools.product(('filled', 'no-fill'), ('literal', 'pending-zero-advance')):
        result.append(dict(mode=mode, start=True, repeat=1, atom=atom, carrier='Lk',
                           active_target='https://ex.org', active_label='K', long_row=True))
    return result


def body_positions(rows):
    """Observe explicit rows; no whitespace folding or row deletion."""
    return [dict(value=match[0], row=index,
                 column=sum(not unicodedata.combining(char) for char in row[:match.start()]),
                 prefix=row[:match.start()], line=row)
            for index, row in enumerate(rows) for match in re.finditer(r'(?:BODY|ODY)_\d+', row)]


def empty_word_identity(metadata):
    if metadata['mode'] == 'entry-control':
        controls = [item for item in empty_word_axes() if item['mode'] == 'entry-control']
        return 'entry-' + str(360 + controls.index(metadata))
    if 'active_label' in metadata:
        return f"visible-link-{metadata['mode']}-{metadata['atom']}"
    if 'visible_target' in metadata:
        return f"active-{metadata['mode']}-{metadata['atom']}"
    if 'control' in metadata:
        return f"flags-{metadata['mode']}-{metadata['start']}-{metadata['control']}-{metadata['atom']}"
    return f"{metadata['mode']}-{metadata['start']}-{metadata['repeat']}-{metadata['carrier']}-{metadata['atom']}"


def validate_empty_word_fixture(fixture, binding=None):
    """Bind finite source axes, raw profiles, BODY ancestry and row evidence."""
    binding = registered_binding() if binding is None else binding
    header, cases = fixture.get('header'), fixture.get('cases')
    require(isinstance(header, dict) and isinstance(cases, list), 'empty words: header/cases shape changed')
    require(header.get('format_version') == 1 and header.get('count') == len(cases) == 472
            and header.get('unique_sources') == 472, 'empty words: identity count changed')
    require(header.get('expectations_from_product') is False
            and header.get('oracle_identity') == binding['identity']
            and header.get('oracle_sha256') == binding['reference_sha256'], 'empty words: oracle binding changed')
    require(all(header.get(key) == count for key, count in dict(
        core_count=360, request_control_count=32, authored_space_count=36,
        active_target_count=4, word_flag_count=36, active_label_count=4).items()),
        'empty words: cohort totals changed')
    require(len({case.get('id') for case in cases}) == 472, 'empty words: duplicate identity')
    expected_axes = {json.dumps(item, sort_keys=True) for item in empty_word_axes()}
    actual_axes = set()
    for case in cases:
        identity, source = case.get('id'), case.get('source')
        require(isinstance(identity, str) and identity and isinstance(source, str)
                and source.endswith('\n') and valid_hash(case.get('source_sha256'))
                and sha(source) == case['source_sha256'], 'empty words: source binding changed')
        metadata = case.get('metadata')
        require(isinstance(metadata, dict) and json.dumps(metadata, sort_keys=True) in expected_axes,
                identity + ': unknown source axes')
        require(source == empty_word_source(metadata) and identity == empty_word_identity(metadata),
                identity + ': identity/axes/source mismatch')
        actual_axes.add(json.dumps(metadata, sort_keys=True))
        require(set(case.get('profile_sha256', {})) == set(PROFILES)
                and all(valid_hash(value) for value in case['profile_sha256'].values()), identity + ': five raw hashes changed')
        statuses = case.get('profile_status', {})
        require(isinstance(statuses, dict) and set(statuses) == set(PROFILES)
                and all(type(code) is int and code in (0, 1, 2) for code in statuses.values()),
                identity + ': five profile statuses changed')
        rows = case.get('expected_rows')
        require(isinstance(rows, list) and all(isinstance(row, str) and '\n' not in row for row in rows),
                identity + ': explicit rows changed')
        uncovered = metadata['carrier'] == 'Sy' and metadata['atom'] == 'combining'
        observation = 'uncovered-bold-combining-backspace' if uncovered else 'asserted'
        require(case.get('row_observation') == observation and any('\b' in row for row in rows) == uncovered,
                identity + ': row observation changed')
        require(case.get('body_positions') == body_positions(rows)
                and len(case['body_positions']) == metadata['repeat'], identity + ': row/column witness changed')
        owners = case.get('ordinary_body_owners')
        require(isinstance(owners, list) and len(owners) == metadata['repeat'], identity + ': BODY owners changed')
        for index, owner in enumerate(owners):
            validate_position(owner, source, identity, require_owner=False)
            require(owner.get('kind') == 'text' and owner.get('label') == f'BODY_{index}'
                    and owner.get('owner') is None and type(owner.get('line_start')) is bool
                    and type(owner.get('no_fill')) is bool and isinstance(owner.get('ancestors'), list)
                    and ['Sh', 'body', 7] in owner['ancestors']
                    and all(parent[0] != 'It' for parent in owner['ancestors'])
                    and source.split('\n')[owner['line'] - 1][owner['column'] - 1:] == owner['label'],
                    identity + ': ordinary Sh BODY ownership changed')
    require(actual_axes == expected_axes and len(actual_axes) == 472
            and len({case['source_sha256'] for case in cases}) == 472, 'empty words: Cartesian source coverage changed')
    require((header.get('asserted_row_count'), header.get('uncovered_row_count')) == (460, 12),
            'empty words: coverage totals changed')
    require(Counter(case['profile_status']['lint'] for case in cases) == {0: 331, 1: 3, 2: 138},
            'empty words: admission totals changed')
    return cases


def validate_live_empty_word_case(case, profiles):
    """Fresh raw/status binding precedes actual Sh BODY and exact row checks."""
    validate_oracle_record(case, dict(source_sha256=case['source_sha256'], **profiles))
    frozen = {name: dict(code=case['profile_status'][name]) for name in PROFILES}
    declared = {'valid': 'legal', 'diagnosed-valid': 'diagnosed', 'recovery': 'recovery'}[admission(frozen)]
    validate_admission(dict(case, oracle_class=declared), profiles)
    for name in PROFILES:
        require(profiles[name]['code'] == case['profile_status'][name]
                and profiles[name]['stdout_sha256'] == case['profile_sha256'][name],
                case['id'] + ': raw ' + name + ' status/hash changed')
    region = select_native_region(profiles['utf8']['stdout'], profiles['tree']['stdout'])
    require(region['status'] == 'asserted', case['id'] + ': native section unavailable')
    # Only the fixed pristine ordinary section's common five-column origin
    # is responsive. SP/NBSP, accepted glyphs and every physical row survive.
    rows = [row[5:] if row.startswith(' ' * 5) else row for row in region['rows']]
    require(rows == case['expected_rows'] and body_positions(rows) == case['body_positions'],
            case['id'] + ': exact native rows/columns changed')
    nodes = fields.tree_owners(profiles['tree']['stdout'])
    actual = [{key: node[key] for key in ('label', 'kind', 'line', 'column', 'line_start', 'no_fill', 'owner', 'ancestors')}
              for node in nodes if node['kind'] == 'text' and node['label'].startswith('BODY_')]
    actual = json.loads(json.dumps(actual))  # normalize ancestry tuples to JSON arrays
    require(actual == case['ordinary_body_owners'], case['id'] + ': actual Sh BODY owner changed')
    return dict(id=case['id'], source_sha256=case['source_sha256'], status='verified',
                row_observation=case['row_observation'], rows=rows)


def capture(reference, source):
    profiles = {}
    for profile in PROFILES:
        arguments = ['-T' + profile] + (['-Owidth=78'] if profile in ('ascii', 'utf8') else [])
        process = run_reference(reference, arguments, input_bytes=source.encode(), timeout=30)
        value = dict(code=process.returncode, timeout=False, arguments=arguments,
                     stdout_sha256=sha(process.stdout), stderr_sha256=sha(process.stderr),
                     stdout_bytes_hex=process.stdout.hex(), stderr_bytes_hex=process.stderr.hex())
        try:
            value.update(stdout=process.stdout.decode('utf8'), stderr=process.stderr.decode('utf8'), utf8_valid=True)
        except UnicodeDecodeError:
            value.update(stdout=process.stdout.decode('utf8', errors='replace'),
                         stderr=process.stderr.decode('utf8', errors='replace'), utf8_valid=False)
        profiles[profile] = value
    return profiles


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', required=True)
    parser.add_argument('--reference', type=Path, default=ROOT / 'target/mandoc-migration/reference/mandoc')
    parser.add_argument('--evidence', type=Path, default=ROOT / 'target/field-supplement-oracle')
    args = parser.parse_args()
    reference = args.reference.resolve()
    registration = verified_reference(ROOT, reference)
    binding = dict(identity=registration['identity'], reference_sha256=sha(reference.read_bytes()))
    fixtures = {name: json.loads((DIRECTORY / (name + '.json')).read_text()) for name in FIXTURES}
    cases = [(name, case) for name, fixture in fixtures.items() for case in validate_fixture(name, fixture, binding)]
    field_count, field_sources = len(cases), len({case['source_sha256'] for _, case in cases})
    empty_fixture = json.loads(EMPTY_WORD_FIXTURE.read_text())
    cases += [('empty_word_columns', case) for case in validate_empty_word_fixture(empty_fixture, binding)]
    sources = {case['source_sha256']: case['source'] for _, case in cases}
    args.evidence.mkdir(parents=True, exist_ok=True)
    cache = {}
    with (args.evidence / 'raw-profiles.jsonl').open('w') as output:
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            for digest, profiles in pool.map(lambda item: (item[0], capture(reference, item[1])), sources.items()):
                cache[digest] = profiles
                output.write(json.dumps(dict(source_sha256=digest, profiles=profiles), ensure_ascii=False) + '\n')
    results = [dict(fixture=name, **(validate_live_empty_word_case(case, cache[case['source_sha256']])
                                   if name == 'empty_word_columns' else validate_live_case(name, case, cache[case['source_sha256']])))
               for name, case in cases]
    manifest = dict(binding, expectations_from_product=False, count=len(cases), unique_sources=len(sources),
                    profiles=list(PROFILES), fixtures={name: sha((DIRECTORY / (name + '.json')).read_bytes()) for name in FIXTURES},
                    raw_profiles_sha256=sha((args.evidence / 'raw-profiles.jsonl').read_bytes()), results=results)
    manifest['fixtures']['empty_word_columns'] = sha(EMPTY_WORD_FIXTURE.read_bytes())
    manifest['cohorts'] = {'field_supplements': dict(count=field_count, unique_sources=field_sources),
                           'empty_word_columns': dict(count=472, unique_sources=472, asserted_rows=460, uncovered_rows=12)}
    (args.evidence / 'manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
    print(f'{len(cases)} identities / {len(sources)} exact sources: five pristine profiles, rows and AST owners verified')


if __name__ == '__main__':
    main()
