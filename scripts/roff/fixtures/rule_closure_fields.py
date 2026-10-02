"""Finite pre-br control inputs from the pinned roff/term rule branches.

Generation defines only sources and required owners. Gold must be acquired
from the registered pristine reference before product assertions are written.
"""

import itertools

HEADER = ('.Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n'
          '.Nm test\n.Nd probe\n.Sh DESCRIPTION\n')
TAIL = '.Sh NEXT\n.No END\n'
WIDTHS = (4, 8, 32)
WORDS = ('no-text', 'empty', 'space', 'tab', 'zero-graph', 'break-point',
         'armed-zero', 'buffered-zero', 'fixed-space', 'nbsp', 'ordinary',
         'continuation', 'word-break', 'zero-graph-break', 'two-words',
         'field-boundary')
CONTROLS = {
    'none': '', 'br': '.br\n', 'nf-fi': '.nf\n.fi\n',
    'nf-br-fi': '.nf\n.br\n.fi\n', 'br-nf-fi': '.br\n.nf\n.fi\n',
    'nf-fi-br': '.nf\n.fi\n.br\n', 'nf-word-fi': '.nf\n.No MID\n.fi\n',
    'nf-zero-fi': '.nf\n.No \\&\n.fi\n',
    'nf-sp1-fi': '.nf\n.sp 1\n.fi\n', 'nf-spneg1-fi': '.nf\n.sp -1\n.fi\n',
    'repeated-fill': '.nf\n.fi\n.nf\n.fi\n', 'nf-font-fi': '.nf\n.ft B\n.fi\n',
    'ti': '.ti\n', 'ti-zero': '.ti +0n\n', 'ti-positive': '.ti 2n\n',
    'ti-negative': '.ti -2n\n', 'ti-fill': '.ti\n.nf\n.fi\n',
    'mc-fill-end': '.mc |\n.nf\n.fi\n.mc\n',
    'nf-mc-fi-end': '.nf\n.mc |\n.fi\n.mc\n',
    'spacing-debt': '.sp 1\n.sp -1\n.sp 1\n',
}


def operand(word, width, carrier='No'):
    if word == 'no-text':
        return ''
    values = {
        'empty': '', 'space': ' ', 'tab': '\t', 'zero-graph': r'\&',
        'break-point': r'\:', 'armed-zero': r'\z', 'buffered-zero': r'\zX',
        'fixed-space': r'\0', 'nbsp': r'\~', 'ordinary': 'A',
        'continuation': r'A\c', 'word-break': r'A\p',
        'zero-graph-break': r'\&\p', 'two-words': 'A B',
        # One identity explicitly covers exact/over-width adjacent words.
        'field-boundary': 'X' * width + ' ' + 'Y' * (width + 1),
    }
    value = values[word]
    if carrier == 'Lk':
        return f'.Lk https://example.org "{value}"\n'
    return f'.{carrier} "{value}"\n'


def source(kind, width, content, *, prefix='', body='.No BodyWord\n', suffix=''):
    if kind == 'column':
        opening = f'.Bl -column "{"x" * width}" "xxxx"\n.It Xo\n'
        closing = '.Xc Ta RIGHT\n'
        body = ''
    elif kind in ('tag', 'hang'):
        opening = f'.Bl -{kind} -width {width}n\n.It Xo\n'
        closing = '.Xc\n'
    elif kind == 'diag':
        # Diag does not parse Xo as an extended HEAD (mdoc_term.c). Its
        # legal label is literal, and subsequent source executes in BODY.
        opening = '.Bl -diag\n.It LABEL\n'
        closing = ''
    else:
        opening = f'.Bl -{kind}\n.It Xo\n'
        closing = '.Xc\n'
    return HEADER + prefix + opening + content + closing + body + '.El\n' + suffix + TAIL


def base_cases():
    """All 2,880 core identities plus directed/shared-state supplements."""
    for index, (kind, word, control, width) in enumerate(itertools.product(
            ('tag', 'hang', 'column'), WORDS, CONTROLS, WIDTHS)):
        yield {
            'id': f'field-pre-br-core-{index:04d}', 'rule_id': ['RC03', 'RC04'],
            'family': 'field-pre-br-core',
            'source': source(kind, width, operand(word, width) + CONTROLS[control]
                             + '.No AFTER\n'),
            'metadata': {'kind': kind, 'word': word, 'control': control,
                         'width': width, 'entry': 'filled',
                         'expected_owner': 'It HEAD' if kind != 'column' else 'It BODY[0]',
                         'reachability': 'requires-pristine-AST-admission',
                         'required_axes': ['content', 'rows', 'separators'],
                         'rule_state': 'NOSPACE -> old field -> pre-br flags'},
        }
    # Pairwise the three independent preceding states with each family,
    # carrier and selected primitive/control branches. Emit explicit pair
    # keys so coverage can be checked rather than inferred from input count.
    states = {'no-fill': '.nf\n', 'closed-row': '.No BEFORE\n.br\n',
              'accepted-prefix': '.No BEFORE\\p\n.No ""\n'}
    for index, (kind, state, carrier, control) in enumerate(itertools.product(
            ('tag', 'hang', 'column', 'ohang', 'inset', 'diag'), states,
            ('No', 'Em', 'Lk'), ('br', 'nf-fi', 'ti', 'nf-word-fi'))):
        yield {
            'id': f'field-pre-br-pairs-{index:04d}', 'rule_id': ['F01', 'F02', 'F04', 'F05', 'F06'],
            'family': 'field-pre-br-pairs',
            'source': source(kind, 8, operand('buffered-zero', 8, carrier)
                             + CONTROLS[control] + '.No AFTER\n', prefix=states[state]),
            'metadata': {'kind': kind, 'precondition': state, 'carrier': carrier,
                         'control': control, 'width': 8,
                         'expected_owner': 'It HEAD' if kind not in ('column', 'diag') else 'It BODY',
                         'reachability': 'requires-pristine-AST-admission',
                         'required_axes': ['content', 'rows', 'separators'],
                         'pair_keys': [[a, b] for a, b in itertools.combinations(
                             (f'kind={kind}', f'state={state}', f'carrier={carrier}',
                              f'control={control}'), 2)]},
        }
    triples = [
        ('continuation-fill-line', 'tag', '.No A\\c\n.nf\n.No MID\n.fi\n.No AFTER\n'),
        ('zero-clear-visible', 'tag', '.No \\&\n.nf\n.fi\n.No AFTER\n'),
        ('column-ti-ta', 'column', '.No A\n.ti\n.No AFTER\n'),
        ('hang-fi-word', 'hang', '.nf\n.No MID\n.fi\n.No AFTER\n'),
        ('pending-zero-generated-owner', 'hang', '.No \\zX\n.mc |\n.Lk https://example.org LABEL\n.mc\n'),
        ('accepted-rejected-wrapper', 'hang', '.Em X\\p\n.Lk https://example.org "\\p Y"\n.No Z\n'),
    ]
    for name, kind, content in triples:
        yield {'id': f'field-pre-br-triple-{name}', 'rule_id': ['F03', 'F04', 'F05', 'F06'],
               'family': 'field-pre-br-triples', 'source': source(kind, 8, content),
               'metadata': {'shared_state_triple': name, 'kind': kind, 'width': 8,
                            'expected_owner': 'It HEAD' if kind != 'column' else 'It BODY[0]',
                            'reachability': 'requires-pristine-AST-admission',
                            'required_axes': ['content', 'rows', 'separators']}}


def supplementary_cases():
    """F07 actual column position/depth; F08 cell-kind/width pair coverage."""
    for index, (count, position, depth) in enumerate(itertools.product(
            (1, 2, 4), ('first', 'middle', 'last'), (1, 2, 4, 8, 16))):
        cell = {'first': 0, 'middle': count // 2, 'last': count - 1}[position]
        widths = ' '.join('"xxxxxxxx"' for _ in range(count))
        before = ' Ta '.join('AUX_LEFT' for _ in range(cell))
        after = ' Ta '.join('AUX_RIGHT' for _ in range(count - cell - 1))
        item = '.It ' + (before + ' Ta ' if before else '') + 'Xo\n'
        content = '.No A\\c\n.ti\n.nf\n.No MID\n.fi\n.No AFTER\n'
        closing = '.Xc' + (' Ta ' + after if after else '') + '\n'
        nested = f'.Bl -column {widths}\n' + item + content + closing + '.El\n'
        for _ in range(depth - 1):
            nested = '.Bl -column "xxxxxxxx" "xxxx"\n.It Xo\n' + nested + '.Xc Ta OUTER\n.El\n'
        yield {'id': f'field-pre-br-columns-{index:04d}', 'rule_id': ['F03', 'F05', 'F07'],
               'family': 'field-pre-br-columns', 'source': HEADER + nested + TAIL,
               'metadata': {'columns': count, 'position': position, 'cell': cell,
                            'depth': depth, 'expected_owner': 'It BODY[requested-cell]',
                            'reachability': 'requires-pristine-AST-admission',
                            'required_axes': ['content', 'rows', 'separators']}}
    words = {'cjk': r'中\[u6587]', 'combining': r'A\[u0301]B',
             'emoji': r'\[u1F642]A', 'tab': 'A\tB', 'author-nbsp': r'A\~B',
             'fixed-blank': r'A\0B', 'multiword': 'AA BB CC'}
    for index, (kind, width, word, control) in enumerate(itertools.product(
            ('tag', 'hang', 'column'), WIDTHS, words, ('br', 'nf-fi', 'ti'))):
        yield {'id': f'field-pre-br-widths-{index:04d}', 'rule_id': ['F02', 'F06', 'F08'],
               'family': 'field-pre-br-widths',
               'source': source(kind, width, f'.No "{words[word]}"\n' + CONTROLS[control] + '.No AFTER\n'),
               'metadata': {'kind': kind, 'width': width, 'word': word, 'control': control,
                            'expected_owner': 'It HEAD' if kind != 'column' else 'It BODY[0]',
                            'reachability': 'requires-pristine-AST-admission',
                            'required_axes': ['content', 'rows', 'separators']}}


def lifecycle_cases():
    """Directed actual macro/owner branches not represented by core W/T."""
    for index, (kind, position, control) in enumerate(itertools.product(
            ('tag', 'hang', 'ohang', 'inset', 'diag', 'column'),
            ('first', 'middle', 'last'), ('br', 'nf-fi', 'ti'))):
        content = '.No A\n' + CONTROLS[control] + '.No AFTER\n'
        one = source(kind, 8, content)
        opening = one[len(HEADER):].split('.It ', 1)[0]
        item = one[len(HEADER) + len(opening):].split('.El\n', 1)[0]
        auxiliary = '.It AUX Ta RIGHT\n' if kind == 'column' else '.It AUX\n.No AUX_BODY\n'
        items = {'first': item + auxiliary + auxiliary,
                 'middle': auxiliary + item + auxiliary,
                 'last': auxiliary + auxiliary + item}[position]
        yield {'id': f'field-pre-br-items-{index:04d}', 'rule_id': ['F01', 'F05', 'F06'],
               'family': 'field-pre-br-items', 'source': HEADER + opening + items + '.El\n' + TAIL,
               'metadata': {'kind': kind, 'position': position, 'control': control,
                            'expected_owner': 'It HEAD/BODY by actual list kind',
                            'reachability': 'requires-pristine-AST-admission',
                            'required_axes': ['content', 'rows', 'separators']}}
    macros = {
        'Nm': '.Nm NAME\n', 'Fn': '.Fn function arg\n',
        'Fo-Fa-Fc': '.Fo call\n.Fa first\n.Fc\n', 'An': '.An Alice\n',
        'Sm': '.Sm off\n.No WORD\n.Sm on\n', 'Ns': '.Ns\n.No WORD\n',
        'Pf': '.Pf PRE WORD\n', 'Lk': '.Lk https://example.org LABEL\n',
    }
    prefixes = {'c': r'A\c', 'p': r'A\p', 'z': r'\zP'}
    for index, (macro, prefix, context, control) in enumerate(itertools.product(
            macros, prefixes, ('paragraph', 'no-fill', 'tag', 'hang', 'column', 'literal'),
            ('br', 'nf-fi', 'ti'))):
        content = f'.No "{prefixes[prefix]}"\n' + macros[macro] + CONTROLS[control] + '.No AFTER\n'
        if context in ('tag', 'hang', 'column'):
            complete = source(context, 8, content)
        elif context == 'literal':
            complete = HEADER + '.Bd -literal\n' + content + '.Ed\n' + TAIL
        else:
            complete = HEADER + ('.nf\n' if context == 'no-fill' else '') + content + TAIL
        yield {'id': f'field-pre-br-macros-{index:04d}', 'rule_id': ['F02', 'F04', 'F05', 'F06'],
               'family': 'field-pre-br-macros', 'source': complete,
               'metadata': {'macro': macro, 'prefix': prefix, 'context': context, 'control': control,
                            'expected_owner': 'actual macro BODY or current owner',
                            'reachability': 'requires-pristine-AST-admission',
                            'required_axes': ['content', 'rows', 'separators']}}
    for index, (macro, prefix, context, control) in enumerate(itertools.product(
            ('UR', 'MT'), prefixes, ('filled', 'no-fill'), ('br', 'nf-fi', 'ti'))):
        target = 'https://example.org' if macro == 'UR' else 'x@example.org'
        end = '.UE' if macro == 'UR' else '.ME'
        prefix_word = prefixes[prefix]
        content = (('.nf\n' if context == 'no-fill' else '') + prefix_word + '\n'
                   + f'.{macro} {target}\nLABEL\n' + CONTROLS[control] + end + '\nAFTER\n')
        complete = '.TH TEST 1 "October 2, 2026"\n.SH DESCRIPTION\n' + content + '.SH NEXT\nEND\n'
        yield {'id': f'field-pre-br-man-links-{index:04d}', 'rule_id': ['F04', 'F05', 'F06'],
               'family': 'field-pre-br-man-links', 'source': complete,
               'metadata': {'macro': macro, 'prefix': prefix, 'context': context, 'control': control,
                            'expected_owner': f'{macro} BODY and post target',
                            'reachability': 'requires-pristine-AST-admission',
                            'required_axes': ['content', 'rows', 'separators']}}
    owners = {
        'literal': '.Bd -literal\n.No INNER\\zX\\c\n.Ed\n',
        'list': '.Bl -inset\n.It INNER\n.No inner-body\\p\n.El\n',
        'table': '.TS\nl l.\nINNER\tCELL\n.TE\n',
        'paragraph': '.Pp\n.No INNER\n',
    }
    for index, (kind, owner, control) in enumerate(itertools.product(
            ('tag', 'hang', 'inset', 'diag'), owners, ('br', 'nf-fi', 'ti'))):
        opening = f'.Bl -{kind}' + (' -width 8n' if kind in ('tag', 'hang') else '') + '\n.It HEAD\n'
        complete = HEADER + opening + '.No X\\p\n' + CONTROLS[control] + owners[owner] + '.No AFTER\n.El\n' + TAIL
        yield {'id': f'field-pre-br-owners-{index:04d}', 'rule_id': ['F02', 'F05', 'F06'],
               'family': 'field-pre-br-owners', 'source': complete,
               'metadata': {'kind': kind, 'owner': owner, 'control': control,
                            'expected_owner': 'It BODY child owner',
                            'reachability': 'requires-pristine-AST-admission',
                            'required_axes': ['content', 'rows', 'separators']}}


def cases():
    yield from base_cases()
    yield from supplementary_cases()
    yield from lifecycle_cases()


def iter_cases():
    return cases()
