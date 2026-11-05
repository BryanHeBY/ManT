"""Finite native enclosure operand/execution contracts, independent of ManT."""

HEADER = ('.Dd November 5, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n'
          '.Nm probe\n.Nd enclosure word execution\n.Sh DESCRIPTION\n')
TAIL = '\n.br\nTAIL\n'


def document(body, no_fill=False):
    return (HEADER + ('.nf\n' if no_fill else '') + 'ORIGIN\n.br\n' + body + TAIL
            + ('.fi\n' if no_fill else '') + '.Sh NEXT\nEND\n')


def wrap(body, context):
    if context == 'keep':
        return '.Bk -words\n' + body + '\n.Ek'
    if context == 'display':
        return '.Bd -filled -compact\n' + body + '\n.Ed'
    if context == 'tag-body':
        return '.Bl -tag -width 4n -compact\n.It TAG\n' + body + '\n.El'
    if context == 'tag-head':
        return '.Bl -tag -width 4n -compact\n.It Xo\n' + body + '\n.Xc\nBODY\n.El'
    return body


def cases():
    result = []
    enclosures = [('eo-empty', '.Eo\n.Ec'),
                  ('en-open-invisible', '.Es \\& ""\n.En'),
                  ('en-close-invisible', '.Es "" \\&\n.En'),
                  ('en-both-invisible', '.Es \\& \\&\n.En')]
    for enclosure, fragment in enclosures:
        for transition, request in [('on', '.Sm on'), ('toggle', '.Sm')]:
            for context in ('plain', 'keep', 'display', 'tag-body'):
                for follower in ('text', 'link'):
                    successor = 'SECOND' if follower == 'text' else '.Lk https://example.test SECOND'
                    body = '.Sm off\n' + fragment + '\n' + request + '\n' + successor
                    result.append(dict(id=f'cursor-{enclosure}-{transition}-{context}-{follower}',
                                       cohort='cursor-core', context=context, no_fill=False,
                                       source=document(wrap(body, context))))
    operands = [('empty', '""'), ('invisible', '\\&'), ('font', '\\fB'),
                ('zero', '\\z'), ('cached', '\\zX'), ('break', '"A\\p"'),
                ('continue', '\\c'), ('punctuation', '['), ('ordinary-punctuation', '@')]
    for label, operand in operands:
        for side in ('opening', 'closing', 'both'):
            opening = operand if side != 'closing' else '""'
            closing = operand if side != 'opening' else '""'
            for no_fill in (False, True):
                body = f'.Es {opening} {closing}\n.En B\nSECOND'
                result.append(dict(id=f'operand-{label}-{side}-' + ('literal' if no_fill else 'filled'),
                                   cohort='raw-operands', context='plain', no_fill=no_fill,
                                   source=document(body, no_fill)))
    for context in ('plain', 'keep', 'display', 'tag-head', 'tag-body'):
        for label, body in [
            ('absent-es', '.En B\nSECOND'),
            ('absent-close', '.Es OPEN\n.En B\nSECOND'),
            ('empty-head', '.Sm off\n.Eo ""\n.Ec\n.Sm on\nSECOND'),
            ('empty-body', '.Sm off\n.Eo\n.No ""\n.Ec\n.Sm on\nSECOND'),
            ('empty-tail', '.Sm off\n.Eo\n.Ec ""\n.Sm on\nSECOND'),
            ('present-parts', '.Eo [\n.No BODY\n.Ec ]\nSECOND'),
            ('immediate-end', '.Sm off\n.Ao\n.Eo\n.Ac\n.Ec\n.Sm on\nSECOND'),
            ('ordinary-empty', '.Sm off\n.No ""\n.Sm on\nSECOND'),
            ('inline-en', '.Es "A\\p" ""\n.No En B\nSECOND'),
            ('font-link', '.Es \\fB ""\n.En\n.Lk https://example.test SECOND'),
        ]:
            result.append(dict(id=f'risk-{label}-{context}', cohort='risk', context=context,
                               no_fill=False, source=document(wrap(body, context))))
    for no_fill in (False, True):
        for label, body in [
            ('eo-control', '.Sm off\n.Eo\n.Ec\n.Sm on\nSECOND'),
            ('true-empty', '.Es "" ""\n.En\nSECOND'),
            ('open-zero-off', '.Sm off\n.Es \\z ""\n.En\n.Sm on\nSECOND'),
            ('close-zero-off', '.Sm off\n.Es "" \\z\n.En\n.Sm on\nSECOND'),
            ('close-cached', '.Es "" \\zX\n.En B\nSECOND'),
            ('previous-glyph', '.No PREFIX\\zX\n.Es \\& ""\n.En B\nSECOND'),
            ('previous-continue', '.No PREFIX\\c\n.Es "" ""\n.En B\nSECOND'),
            ('font-previous', '.ft I\n.Es \\fB \\fP\n.En B\nSECOND'),
            ('eo-on-off', '.Sm on\n.Eo\n.Ec\n.Sm off\nSECOND'),
            ('eo-repeated-off', '.Sm off\n.Eo\n.Ec\n.Sm off\nSECOND'),
            ('en-on-off', '.Sm on\n.Es \\& \\&\n.En\n.Sm off\nSECOND'),
            ('en-repeated-off', '.Sm off\n.Es \\& \\&\n.En\n.Sm off\nSECOND'),
        ]:
            result.append(dict(id=f'control-{label}-' + ('literal' if no_fill else 'filled'),
                               cohort='controls', context='plain', no_fill=no_fill,
                               source=document(body, no_fill)))
    return result
