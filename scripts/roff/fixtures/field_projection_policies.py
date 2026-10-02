"""Exact-source presentation cards for the pre-br field matrix.

These cards observe pristine tree/output facts. They do not execute roff,
change native word acceptance, erase author spaces, or waive a matrix family.
"""

import gzip
import hashlib
import json
from pathlib import Path
import re

from scripts.roff.fixtures.acceptance_regions import select_native_region
from scripts.roff.lib.roff_content_compare import visible_text

FIXTURE = (Path(__file__).resolve().parents[3] / 'crates/mant-engine/tests/'
           'roff_lowering/control_request_matrix/cases.jsonl.gz')


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def tree_owners(tree):
    """Observe It HEAD/BODY ancestry and exact TEXT/control positions."""
    stack, result = [], []
    for raw in tree.splitlines():
        match = re.match(r'^( *)(.*?) \((block|head|body|elem|text|tbl)\).*?(\*?)(\d+):(\d+)(?: |$)', raw)
        if not match:
            continue
        indent, label, kind, line_start, line, column = match.groups()
        depth = len(indent)
        while stack and stack[-1]['depth'] >= depth:
            stack.pop()
        if kind == 'text' and stack:
            # print_tree indents children by four cells. Any additional
            # leading SP belongs to the TEXT payload (not an empty operand).
            payload_spaces = max(0, depth - stack[-1]['depth'] - 4)
            label = ' ' * payload_spaces + label
            depth -= payload_spaces
        owner = next((node for node in reversed(stack)
                      if node['label'] == 'It' and node['kind'] in ('head', 'body')), None)
        node = {'label': label, 'kind': kind, 'line': int(line),
                'column': int(column), 'depth': depth,
                'line_start': bool(line_start), 'no_fill': ' NOFILL' in raw,
                'owner': ('It ' + owner['kind'].upper()) if owner else None,
                'ancestors': [(parent['label'], parent['kind'], parent['line'])
                              for parent in stack]}
        result.append(node)
        stack.append(node)
    return result


def owner_witness(case, tree):
    nodes = tree_owners(tree)
    probe = [node for node in nodes if node['kind'] == 'text' and node['label'] == 'AFTER']
    expected = case['metadata'].get('expected_owner', '')
    if not probe and case['family'] == 'field-pre-br-triples':
        probe = [node for node in nodes if node['kind'] == 'text'
                 and node['label'] in ('LABEL', 'Z')]
    if expected == 'It HEAD/BODY by actual list kind':
        expected = 'It BODY' if case['metadata']['kind'] in ('column', 'diag') else 'It HEAD'
    if expected.startswith('It HEAD'):
        reachable = bool(probe) and all(node['owner'] == 'It HEAD' for node in probe)
    elif expected.startswith('It BODY'):
        reachable = bool(probe) and all(node['owner'] == 'It BODY' for node in probe)
    else:
        reachable = False
    branch = []
    metadata = case['metadata']
    if case['family'] == 'field-pre-br-macros':
        macro = metadata['macro']
        required = [('Fo', 'block'), ('Fo', 'head'), ('Fo', 'body'), ('Fa', 'elem')] if macro == 'Fo-Fa-Fc' else [(macro, 'elem')]
        lines = {index for index, line in enumerate(case['source'].splitlines(), 1)
                 if index > 7 and any(line.startswith('.' + name + ' ')
                                      or line == '.' + name for name, _ in required)}
        branch = [node for node in nodes if (node['label'], node['kind']) in required
                  and node['line'] in lines]
        reachable = all(any((node['label'], node['kind']) == item for node in branch)
                        for item in required) and bool(probe)
        context = metadata['context']
        if context in ('tag', 'hang', 'column'):
            wanted = 'It BODY' if context == 'column' else 'It HEAD'
            reachable &= all(node['owner'] == wanted for node in branch + probe)
        elif context == 'literal':
            reachable &= all(any(label == 'Bd' and kind == 'body'
                                  for label, kind, _ in node['ancestors'])
                             for node in branch + probe if node['kind'] != 'head')
        else:
            reachable &= all(any(label == 'Sh' and kind == 'body' and line == 7
                                  for label, kind, line in node['ancestors'])
                             for node in branch + probe)
        if context == 'no-fill':
            reachable &= all(node['no_fill'] for node in branch)
        if macro == 'Fo-Fa-Fc':
            reachable &= any(node['label'] == 'Fa' and any(label == 'Fo' and kind == 'body'
                              for label, kind, _ in node['ancestors']) for node in branch)
            # Fc normally closes the existing Fo BODY without a separate AST
            # node. The following actual control/AFTER must lie outside it.
            reachable &= all(not any(label == 'Fo' for label, _, _ in node['ancestors'])
                             for node in probe)
    if case['family'] == 'field-pre-br-owners':
        owner = metadata['owner']
        name, kind = {'literal': ('Bd', 'block'), 'list': ('Bl', 'block'),
                      'table': (None, 'tbl'), 'paragraph': ('Pp', 'elem')}[owner]
        branch = [node for node in nodes if node['kind'] == kind
                  and (name is None or node['label'] == name) and node['owner'] == 'It BODY']
        reachable &= bool(branch)
    if case['family'] == 'field-pre-br-columns':
        # Nested columns have multiple AFTER probes; the deepest It BODY
        # remains the requested actual cell, never an extended column HEAD.
        reachable = bool(probe) and all(node['owner'] == 'It BODY' for node in probe)
    if case['family'] == 'field-pre-br-man-links':
        macro = case['metadata']['macro']
        branch = [node for node in nodes if node['label'] == macro
                  and node['kind'] in ('block', 'head', 'body')]
        reachable = {node['kind'] for node in branch} == {'block', 'head', 'body'}
        reachable &= any(node['label'] == 'LABEL' and any(label == macro and kind == 'body'
                            for label, kind, _ in node['ancestors']) for node in nodes)
    texts = [node for node in nodes if node['kind'] == 'text']
    return {'reachable': reachable, 'expected_owner': expected,
            'after_owners': [{'line': node['line'], 'column': node['column'],
                              'owner': node['owner'], 'no_fill': node['no_fill']}
                             for node in probe],
            'empty_texts': sum(node['label'] == '' for node in texts),
            'literal_tabs': [{'line': node['line'], 'column': node['column'],
                              'owner': node['owner']} for node in texts if '\t' in node['label']],
            'no_text_operand': case['metadata'].get('word') == 'no-text',
            'branch_nodes': [{key: node[key] for key in ('label', 'kind', 'line', 'column', 'owner', 'no_fill')}
                             for node in branch]}


def build_policy(case, oracle, binding):
    """Derive a bound card from source/AST/pristine output, never candidate."""
    source, tree, raw = case['source'], oracle['tree']['stdout'], oracle['utf8']['stdout']
    card = {'id': case['id'], 'source_sha256': digest(source),
            'oracle_identity': binding['identity'],
            'oracle_sha256': binding['reference_sha256'],
            'native_utf8_sha256': digest(raw), 'native_tree_sha256': digest(tree),
            'indent': 'responsive', 'padding': 'responsive', 'tab': 'device-expansion',
            'native_replacements': [], 'native_row_joins': [], 'product_table_seams': [],
            'rules': []}
    nodes = tree_owners(tree)
    pipe_texts = [node for node in nodes if node['kind'] == 'text' and '|' in node['label']]
    mc_lines = [index for index, line in enumerate(source.splitlines(), 1)
                if line.startswith('.mc')]
    mc_nodes = [node for node in nodes if node['kind'] == 'elem' and node['label'] == 'mc']
    if (len(mc_lines) == len(mc_nodes) == 2
            and source.splitlines()[mc_lines[0] - 1] == '.mc |'
            and source.splitlines()[mc_lines[1] - 1] == '.mc'
            and len(pipe_texts) == 1 and pipe_texts[0]['label'] == '|'
            and pipe_texts[0]['line'] == mc_lines[0]
            and source.count('|') == 1):
        tails = [{'row': index, 'column': len(row) - 1, 'glyph': '|'}
                 for index, row in enumerate(visible_text(raw).splitlines()) if row.endswith('|')]
        if tails and all(tail['column'] >= 79 for tail in tails):
            card['margin_policy'] = {key: card[key] for key in
                                     ('native_utf8_sha256', 'native_tree_sha256')}
            card['margin_policy']['tail_cells'] = tails
            card['rules'].append('term.c:450-465 installed mc endline cell only')
    region = select_native_region(raw, tree, margin_policy=card.get('margin_policy'))
    card['original_region_status'] = select_native_region(raw, tree)['status']
    if region['status'] != 'asserted':
        return card, region
    rows = region['rows']
    # termp_it_pre() generates one escaped U+00A0 for inset/diag BODY.
    # No author spelling of any nonbreaking space is allowed by this card.
    run_in = any(re.search(r'^\s*Bl \(block\) -(?:inset|diag)(?: |$)', row)
                 for row in tree.splitlines())
    author_nbsp = any(token in source for token in ('\\~', '\\0', '\\ ', '\u00a0', '\\[u00A0]', '\\[u00a0]'))
    if run_in and not author_nbsp:
        card['native_replacements'] = [{'row': row_index, 'column': column,
                                        'from': '\u00a0', 'to': ' '}
                                       for row_index, row in enumerate(rows)
                                       for column, ch in enumerate(row) if ch == '\u00a0']
        if card['native_replacements']:
            card['rules'].append('mdoc_term.c:764 generated inset/diag BODY separator')
    # Literal tabs stay in the IR. Its declared-column presentation is
    # responsive; only these observed RIGHT row tails are device width ends.
    # No source hard break/no-fill BODY entry occurs at the RIGHT ownership.
    metadata = case['metadata']
    tab_column = metadata.get('kind') == 'column' and (
        (case['family'] == 'field-pre-br-core' and metadata.get('word') == 'tab'
         and metadata.get('width') == 4 and metadata.get('control') == 'none')
        or (case['family'] == 'field-pre-br-widths' and metadata.get('word') == 'tab'
            and metadata.get('width') == 8))
    right = [node for node in nodes if node['kind'] == 'text' and node['label'] == 'RIGHT']
    tab_nodes = [node for node in nodes if node['kind'] == 'text' and '\t' in node['label']]
    if (tab_column and len(right) == len(tab_nodes) == 1
            and right[0]['owner'] == 'It BODY' and not right[0]['no_fill']
            and rows[-1].lstrip(' ') == 'RIGHT' and len(rows) >= 2):
        card['native_row_joins'] = [{'row': len(rows) - 1, 'content': 'RIGHT'}]
        card['literal_tab_sites'] = [{'row': 0, 'scalar_boundary':
                                     1 if tab_nodes[0]['label'].startswith('A') else 0}]
        card['rules'].append('mdoc_term.c:953 / term.c:250-253 responsive declared-column width tail')
    if metadata.get('owner') == 'table' and '\n.TS\nl l.\nINNER\tCELL\n.TE\n' in source:
        witnesses = [index for index, row in enumerate(rows)
                     if re.fullmatch(r' *INNER +CELL *', row)]
        if len(witnesses) == 1 and '(tbl)' in tree:
            card['product_table_seams'] = [{'row': witnesses[0], 'left': 'INNER', 'right': 'CELL'}]
            card['rules'].append('tbl_term.c native columns / IR table textual separator')
    return card, region


def load_policies(path=FIXTURE):
    with gzip.open(path, 'rt', encoding='utf-8') as source:
        next(source)  # oracle/count header
        return {row['id']: row['projection'] for row in map(json.loads, source)}


def qualified_policy(case, oracle, binding, policies=None):
    policies = load_policies() if policies is None else policies
    card = policies.get(case['id'])
    if card is None:
        return {}, 'missing exact field presentation card'
    expected = {'source_sha256': digest(case['source']),
                'oracle_identity': binding['identity'],
                'oracle_sha256': binding['reference_sha256'],
                'native_utf8_sha256': digest(oracle['utf8']['stdout']),
                'native_tree_sha256': digest(oracle['tree']['stdout'])}
    mismatch = [key for key, value in expected.items() if card.get(key) != value]
    return ({}, 'field card binding changed: ' + ', '.join(mismatch)) if mismatch else (card, None)


def project_regions(native, product, card):
    """Apply precise observed display edits; all other rows/scalars survive."""
    if native.get('status') != 'asserted' or product.get('status') != 'asserted':
        return native, product
    native, product = dict(native, rows=list(native['rows'])), dict(product, rows=list(product['rows']))
    for change in card.get('native_replacements', []):
        row, column = change['row'], change['column']
        value = native['rows'][row]
        if value[column:column + 1] != change['from']:
            raise ValueError('generated native separator coordinate changed')
        native['rows'][row] = value[:column] + change['to'] + value[column + 1:]
    for change in reversed(card.get('native_row_joins', [])):
        row = change['row']
        if row == 0 or native['rows'][row].lstrip(' ') != change['content']:
            raise ValueError('responsive column tail coordinate changed')
        native['rows'][row - 1] = native['rows'][row - 1].rstrip(' ') + ' ' + change['content']
        del native['rows'][row]
    for seam in card.get('product_table_seams', []):
        row = seam['row']
        pattern = r'^( *)' + re.escape(seam['left']) + r' +\| +' + re.escape(seam['right']) + r' *$'
        match = re.fullmatch(pattern, product['rows'][row])
        if match is None:
            raise ValueError('generated table separator owner changed')
        product['rows'][row] = match[1] + seam['left'] + ' ' + seam['right']
    return native, product
