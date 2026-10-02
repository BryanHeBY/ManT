"""Finite sources derived from roff_escape.c syntax and delimiter branches.

Source generation never executes ManT and never produces expected output.
Two identities may share a source when they exercise both delimiter positions;
callers must report unique sources separately.
"""

from itertools import product

HEADER = '.Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
MAN_HEADER = '.TH TEST 1 "October 2, 2026" "Fixed Oracle Footer"\n.SH DESCRIPTION\n'
TAIL = '.Sh NEXT\n.No END\n'


def escape_rule_cases():
    cases = []

    def add(rule, name, word, carrier='No', suffix='.No AFTER\n', metadata=None):
        body = (f'.Lk https://example.org "{word}"\n' if carrier == 'Lk'
                else f'.{carrier} "{word}"\n')
        cases.append(dict(name=f'{rule.lower()}_{name}_{carrier.lower()}',
                          source=HEADER + body + suffix + TAIL,
                          rule_id=rule, carrier=carrier,
                          axes=['content', 'rows', 'separators'],
                          metadata=metadata or {}))

    # A standard argument's three native shapes. Font operands are valid;
    # ignore-family names need not refer to installed device resources.
    for trigger, shape, outer, position, carrier in product(
            'FMYkmf', ['single', 'two', 'bracket'], 'oXZ',
            ['start', 'internal', 'end'], ['No', 'Lk']):
        operand = {'single': 'B', 'two': '(BI', 'bracket': '[BI]'}[shape]
        control = '\\' + trigger + operand
        if position == 'internal':
            word = f'A\\{outer}|X{control}Y|Z'
        else:
            # Same syntax proves opening/closing trigger identity separately.
            # Both identities remain in the ledger, never counted as unique.
            word = f'A\\{outer}{control}XY{control}Z'
        add('E02', f'{trigger}_{shape}_{outer}_{position}', word, carrier,
            metadata=dict(trigger=trigger, shape=shape, outer=outer, position=position))

    for trigger, outer, position, carrier in product(
            'FMYkmf', 'oXZ', ['start', 'internal', 'end'], ['No', 'Lk']):
        control = '\\' + trigger + '[]'
        word = (f'A\\{outer}|X{control}Y|Z' if position == 'internal'
                else f'A\\{outer}{control}XY{control}Z')
        add('E02', f'{trigger}_empty_{outer}_{position}', word, carrier,
            metadata=dict(trigger=trigger, shape='empty', outer=outer, position=position))

    sizes = ['1', '12', '24', '34', '40', '(12', '[12]', "'12'", "'1\\&2'"]
    for sign, size, outer, delimiter, carrier, copy in product(
            ['', '+', '-'], sizes, 'oXZ', ["'", '|'], ['No', 'Lk'], [False, True]):
        control = ('\\Es' if copy else '\\s') + sign + size
        word = f'A\\{outer}{delimiter}X{control}Y{delimiter}Z'
        add('E01', f'{len(cases):04}_{outer}', word, carrier,
            metadata=dict(sign=sign, size=size, outer=outer, delimiter=delimiter, copy=copy))

    for size, sign, outer, carrier, copy in product(
            ["''", '[]', '(', '[12', "'12", ''], ['', '+', '-'],
            'oXZ', ['No', 'Lk'], [False, True]):
        control = ('\\Es' if copy else '\\s') + sign + size
        add('E01', f'incomplete_{len(cases):04}', f'A\\{outer}|X{control}', carrier,
            metadata=dict(size=size, sign=sign, completion='empty-or-unclosed', copy=copy))

    # Counted operands skip a complete nested escape without decrementing
    # maxl. Cover this branch for each size shape, including the undecided
    # one-unit shape: a backslash does not become a size delimiter.
    for size, outer, delimiter, carrier in product(
            ['\\&1', '+\\&1', '(1\\&2', '[1\\&2]', "'1\\E&2'", '\\(aq12\\(aq'],
            'oXZ', ["'", '|'], ['No', 'Lk']):
        word = f'A\\{outer}{delimiter}X\\s{size}Y{delimiter}Z'
        add('E01', f'nested_count_{len(cases):04}', word, carrier,
            metadata=dict(size=size, outer=outer, delimiter=delimiter, nested_count=True))

    delimiters = ["'", '|', '\\(aq', '\\[aq]', '\\m[red]', '\\fB']
    for outer, delimiter, completion, carrier in product(
            'oXZChN', delimiters, ['closed', 'opening', 'unclosed'], ['No', 'Lk']):
        payload = '65' if outer == 'N' else 'em' if outer == 'C' else '0n' if outer == 'h' else 'XY'
        middle = '' if completion == 'opening' else payload
        close = delimiter + 'Z' if completion == 'closed' else ''
        add('E03', f'{len(cases):04}_{outer}_{completion}',
            f'A\\{outer}{delimiter}{middle}{close}', carrier,
            metadata=dict(outer=outer, delimiter=delimiter, completion=completion))

    for depth, inner, outer, carrier in product(
            range(4), ['size', 'ignore', 'named', 'unknown'], 'oXZ', ['No', 'Lk']):
        payload = {'size': "X\\s'1\\&2'Y", 'ignore': 'X\\m[red]Y',
                   'named': 'X\\(aqY', 'unknown': 'X\\qY'}[inner]
        for _ in range(depth):
            payload = '\\Z|' + payload + '|'
        add('E04', f'{depth}_{inner}_{outer}', f'A\\{outer}\'( {payload} )\'Z', carrier,
            metadata=dict(depth=depth, inner=inner, outer=outer))

    for prefix, suffix, outer, carrier in product(
            ['', '\\z', '\\zP', 'A\\c', 'A\\p'],
            ['Z', 'line', 'link', 'owner'], 'oXZ', ['No', 'Lk']):
        tail = {'Z': '.No AFTER\n', 'line': '.br\n.No AFTER\n',
                'link': '.Lk https://after.example.org AFTER\n',
                'owner': '.Pp\n.No AFTER\n'}[suffix]
        word = prefix + '\\' + outer + "|X\\s'12'Y|" + ('Z' if suffix == 'Z' else '')
        add('E05', f'{len(cases):04}_{outer}_{suffix}', word, carrier, tail,
            metadata=dict(prefix=prefix, suffix=suffix, outer=outer))

    for trigger, operand, outer, carrier in product(
            ['O', 'q'], ['1', '2', '[abc]', ''], 'oXZ', ['No', 'Lk']):
        control = '\\' + trigger + operand
        add('E02', f'control_{trigger}_{len(cases):04}',
            f'A\\{outer}{control}XY{control}Z', carrier,
            metadata=dict(trigger=trigger, operand=operand, outer=outer))

    words = ["A\\o'X\\s'12'Y'Z", 'A\\o\\m[red]XY\\m[blue]Z',
             'A\\X\\m[red]PAYLOAD\\m[blue]Z', "A\\Z|X\\s+'12'Y|Z"]
    for carrier, word in product(['No', 'Em', 'Sy', 'Lk', 'head', 'literal',
                                  'TEXT', 'B', 'I', 'BR', 'BI', 'UR'], words):
        name = f'e06_{len(cases):04}_{carrier.lower()}'
        if carrier in ['No', 'Em', 'Sy', 'Lk']:
            add('E06', name, word, carrier)
        else:
            if carrier == 'head':
                source = HEADER + f'.Bl -tag -width 4n\n.It "{word}"\n.No AFTER\n.El\n' + TAIL
            elif carrier == 'literal':
                source = HEADER + f'.Bd -literal -compact\n.No "{word}"\n.No AFTER\n.Ed\n' + TAIL
            else:
                body = (word + '\n' if carrier == 'TEXT' else
                        f'.UR https://example.org\n{word}\n.UE\n' if carrier == 'UR' else
                        f'.{carrier} "{word}"\n')
                source = MAN_HEADER + body + 'AFTER\n.SH NEXT\nEND\n'
            cases.append(dict(name=name, source=source, rule_id='E06', carrier=carrier,
                              axes=['content', 'rows', 'separators'], metadata={}))

    for depth, closed in product([1, 2, 8, 64, 255, 256, 257, 300], [False, True]):
        word = 'A' + "\\X'" * depth + 'Q' + ("'" * depth + 'Z' if closed else '')
        add('E07', f'depth_{depth}_{closed}', word,
            metadata=dict(depth=depth, closed=closed, budget=depth > 256))
    for count, closed in product([1024, 2048, 4096, 8192], [False, True]):
        add('E07', f'long_{count}_{closed}', 'A\\X|' + 'q' * count + ('|Z' if closed else ''),
            metadata=dict(length=count, closed=closed))
    # Immutable independently named original findings survive generator changes.
    for name, word in [('size_literal_quote', words[0]), ('ignore_overstrike', words[1]),
                       ('ignore_postprocessor', words[2])]:
        add('RC01' if name == 'size_literal_quote' else 'RC02', name, word)
    # Append-only postclassification cohort: mandatory EOF is a pre-scan
    # return in roff_escape.c, distinct from a payload that fails to close.
    # Original identities above stay fixed when adjacent branches are added.
    def boundary(rule, trigger, operand, prefix, carrier, branch):
        word = prefix + '\\' + trigger + operand
        body = f'.No "{word}"\n' if carrier == 'No' else word + '\n'
        cases.append(dict(name=f'{rule.lower()}_boundary_{len(cases):04}_{carrier.lower()}',
                          source=HEADER + '.nf\n' + body + '.No AFTER\n.fi\n' + TAIL,
                          rule_id=rule, carrier=carrier,
                          axes=['content', 'rows', 'separators'],
                          metadata=dict(branch=branch, trigger=trigger, operand=operand,
                                        prefix=prefix, source_context='no-fill')))

    for sign, opening, payload, prefix, carrier in product(
            ['', '+', '-'], ['', '(', '[', "'"], ['', '1'], ['', '\\z'], ['No', 'TEXT']):
        boundary('E01', 's', sign + opening + payload, prefix, carrier, 'mandatory-or-unclosed')
    for trigger, operand, prefix, carrier in product(
            'DHLRSXZbvx', ['', "'", "'A", "'A'"], ['', '\\z'], ['No', 'TEXT']):
        boundary('E03', trigger, operand, prefix, carrier, 'mandatory-or-unclosed')
    for trigger, operand, prefix, carrier in product(
            'FMYkmO', ['', '[', '(', '[B', '(B', '[]', '[B]'], ['', '\\z'], ['No', 'TEXT']):
        boundary('E02', trigger, operand, prefix, carrier, 'standard-eof-extent')
    for operand in ['', '[', '(', '[BI', '(B', '[garbage]', '[]', '[BI]', '(BI',
                    'B', 'I', 'P', 'R', '1', '2', '3', '4', 'C', 'V',
                    '[CB]', '[CI]', '[CR]', '[CW]', '[VB]', '[VI]', '[VR]', '[X]']:
        for carrier in ['TEXT', 'B', 'I']:
            word = 'A\\f' + operand
            body = word + '\n' if carrier == 'TEXT' else f'.{carrier} "{word}"\n'
            cases.append(dict(name=f'e02_font_postclass_{len(cases):04}_{carrier.lower()}',
                              source=MAN_HEADER + '.ft I\n.ft B\n' + body + 'AFTER\n.SH NEXT\nEND\n',
                              rule_id='E02', carrier=carrier,
                              axes=['content', 'rows', 'separators', 'style'],
                              metadata=dict(branch='font-postclass', trigger='f', operand=operand,
                                            font_extension=operand in ['C', 'V', '[VB]', '[VI]'])))
    return cases


def cases():
    """Shared replay adapter: identities preserve exact generated sources."""
    return [dict(case, id=case['name'], family='escape-rule-grammar')
            for case in escape_rule_cases()]
