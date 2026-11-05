"""Finite complete Nm sessions, independently from their unsupported tbl form."""

HEADER = ('.Dd November 5, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n'
          '.Nd test\n.Sh SYNOPSIS\n')
TAIL = '.Sh NEXT\nAFTER.\n'
HEADS = {'implicit': '.Nm', 'plain': '.Nm probe', 'short': '.Nm p',
         'long': '.Nm longcommandname', 'multi': '.Nm probe extra'}
BODIES = {'text': 'int', 'Ft': '.Ft int', 'Ar': '.Ar FILE',
          'Fl': '.Fl a', 'No': '.No int'}


def document(body):
    return HEADER + body + '\n' + TAIL


def table(fragment):
    return '.TS\nl.\nT{\n' + fragment + '\nT}\n.TE'


def cases():
    result = []
    for head, request in HEADS.items():
        for body, content in BODIES.items():
            result.append(dict(id=f'core-{head}-{body}', cohort='core',
                               fragment=request + '\n' + content))
    risks = [
        ('empty-body-implicit', '.Nm'),
        ('empty-body-plain', '.Nm probe'),
        ('same-line-multi', '.Nm probe tail'),
        ('body-empty', '.Nm probe\n.No ""'),
        ('body-invisible', '.Nm probe\n.No \\&'),
        ('body-external', '.Nm probe\n.Lk https://example.test LABEL'),
        ('body-manual', '.Nm probe\n.Xr printf 3'),
        ('prefix-Fa', '.Fa arg\n.Nm probe\n.Ft int'),
        ('prefix-No', '.No arg\n.Nm probe\n.Ft int'),
        ('prefix-Fn', '.Fn call\n.Nm probe\n.Ft int'),
        ('prefix-bare-zero', '.No \\z\n.Nm probe\n.Ft int'),
        ('prefix-cached-zero', '.No \\zX\n.Nm probe\n.Ft int'),
        ('prefix-continuation', '.No arg\\c\n.Nm probe\n.Ft int'),
        ('prefix-empty', '.No ""\n.Nm probe\n.Ft int'),
        ('body-suffix', '.Nm probe\n.Ft int\n.No SUFFIX'),
        ('new-name', '.Nm probe\n.Ft int\n.Nm next\n.Ar FILE'),
        ('spacing-off', '.Sm off\n.Nm probe\n.Ft int'),
        ('enclosure', '.Bo\n.Nm probe\n.Ft int\n.Bc'),
        ('head-font', '.Nm "\\fIprobe"\n.Ft int'),
        ('body-pending-zero', '.Nm probe\n.Ft "int\\z"\n.No NEXT'),
    ]
    result.extend(dict(id='risk-' + label, cohort='risk', fragment=fragment)
                  for label, fragment in risks)
    for case in result:
        case['source'] = document(table(case['fragment']))
        case['independent_source'] = document(case['fragment'])
    # These are ordinary nested executions, not an expansion of the bounded
    # cell language: neither Fo nor Bl is admitted by source_fragment.
    for index, fragment in enumerate(('.Nm\n.Ft int', '.Nm probe\n.Ft int',
                                      '.Nm probe tail',
                                      '.Nm probe\n.Ft "int\\z"\n.No NEXT')):
        for context in ('head', 'Fo'):
            body = ('.Bl -tag -width 20n\n.It Xo\n' + fragment + '\n.Xc\nBODY\n.El'
                    if context == 'head' else '.Fo func\n' + fragment + '\n.Fc')
            result.append(dict(id=f'nested-{context}-{index}', cohort='nested',
                               fragment=fragment, source=document(body),
                               independent_source=None))
    for style in ('tag', 'hang', 'inset'):
        width = ' -width 20n' if style != 'inset' else ''
        body = f'.Bl -{style}{width}\n.It key\n.Nm probe\n.Ft int\n.El'
        result.append(dict(id='nested-body-' + style, cohort='nested', fragment=None,
                           source=document(body), independent_source=None))
    return result


def cell_controls():
    result = []
    for label, first, second, adjacent in [
        ('adjacent', '.Nm probe\n.Ft int', '.No CELL', True),
        ('following-row', '.Nm probe\n.Ft int', '.Nm next\n.Ar FILE', False),
        ('cached-zero', '.Nm probe\n.Ft "int\\z"', '.No CELL', True),
        ('link-owner', '.Nm probe\n.Lk https://example.test LABEL', '.Nm next\n.Ft int', True),
    ]:
        columns = 'l l.' if adjacent else 'l.'
        boundary = 'T}\tT{\n' if adjacent else 'T}\nT{\n'
        body = f'.TS\n{columns}\nT{{\n{first}\n{boundary}{second}\nT}}\n.TE'
        result.append(dict(id='cells-' + label, source=document(body), adjacent=adjacent,
                           fragments=[document(first), document(second)]))
    return result
