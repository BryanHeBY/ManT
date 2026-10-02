"""Finite controlled-omission inputs; no candidate output defines gold.

Depth assertions follow the locked native guard in roff_escape_impl(),
independently of severity and of the human-readable mandoc message.
"""

import itertools

HEADER = ('.Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n'
          '.Nm test\n.Nd probe\n.Sh DESCRIPTION\n')
TAIL = '.Sh NEXT\n.No END\n'


def cases():
    """Yield boundary, recovery, ownership and no-loss controls."""
    for depth, closed, prefix, carrier in itertools.product(
            (1, 2, 8, 64, 255, 256, 257, 300), (False, True),
            ('A', 'αe\u0301'), ('No', 'Lk', 'TEXT')):
        word = prefix + r"\X'" * depth + 'Q'
        if closed:
            word += "'" * depth + 'Z'
        if carrier == 'TEXT':
            source = ('.TH TEST 1 "October 2, 2026" "Fixed Oracle Footer"\n'
                      '.SH DESCRIPTION\n' + word + '\nAFTER\n.SH NEXT\nEND\n')
        else:
            macro = '.No' if carrier == 'No' else '.Lk https://example.org'
            source = HEADER + f'{macro} "{word}"\n.No AFTER\n' + TAIL
        yield {
            'id': f'escape-coverage-{depth}-{int(closed)}-{carrier.lower()}-'
                  + ('ascii' if prefix == 'A' else 'unicode'),
            'rule_id': 'D01', 'family': 'escape-coverage', 'source': source,
            'metadata': {
                'depth': depth, 'closed': closed, 'prefix': prefix,
                'carrier': carrier, 'native_guard_expected': depth > 256,
                'required_axes': ['content-coverage', 'semantics-coverage',
                                  'later-line', 'json', 'query', 'session-isolation'],
                'rule': 'roff_escape_impl depth >= 256 consumes rejected suffix',
            },
        }
    controls = {
        'ordinary-style-warning': '.No A\n',
        'ignored-device-control': r'.No "A\X|DEVICE|Z"' + '\n',
        'font-warning': r'.No "A\f[unavailable]Z"' + '\n',
        'source-spelling-fallback': r'.No "A\N|65535|Z"' + '\n',
        'unknown-escape': r'.No "A\qZ"' + '\n',
        'unclosed-control': r'.No "A\X|DEVICE"' + '\n',
        'zero-width-word': r'.No "A\&Z"' + '\n',
    }
    for name, body in controls.items():
        yield {'id': f'coverage-control-{name}', 'rule_id': 'D03',
               'family': 'coverage-controls',
               'source': HEADER + body + '.No AFTER\n' + TAIL,
               'metadata': {'native_guard_expected': False,
                            'required_axes': ['content-coverage', 'semantics-coverage',
                                              'later-line', 'json', 'query']}}
    for depth, owner in itertools.product((255, 256, 257, 300),
                                          ('native-cell', 'font-cell', 'source-fragment')):
        word = 'A' + r"\X'" * depth + 'Q' + "'" * depth + 'Z'
        if owner == 'source-fragment':
            cell = 'T{\n.No "' + word + '"\nT}\n'
        elif owner == 'font-cell':
            cell = r'\fB' + word + r'\fR' + '\n'
        else:
            cell = word + '\n'
        yield {'id': f'coverage-table-{owner}-{depth}', 'rule_id': 'D04',
               'family': 'coverage-table-owners',
               'source': HEADER + '.TS\nl.\n' + cell + '.TE\n.No AFTER\n' + TAIL,
               'metadata': {'depth': depth, 'owner': owner, 'prefix': 'A',
                            'native_guard_expected': depth > 256,
                            'required_axes': ['content-coverage', 'semantics-coverage',
                                              'later-line', 'json', 'query', 'table-owner'],
                            'rule': 'roff_expand/tbl_read and scoped tbl_word execute the same bounded escape grammar'}}


iter_cases = cases
