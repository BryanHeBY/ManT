"""Finite parent-sequence contracts for recovered cells and Rs child owners.

A table and its independent fragment are separate oracle inputs. The former
records UNSUPP; only the latter defines the admitted inline recovery contract.
No product output participates in either source or expected-result generation.
"""

HEADER = ('.Dd November 4, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n'
          '.Nm probe\n.Nd source adjacency review\n.Sh {section}\n')
TAIL = '.Sh NEXT\nAFTER.\n'


def document(body, section='SYNOPSIS'):
    return HEADER.format(section=section) + body + '\n' + TAIL


def table(fragment):
    return '.TS\nl.\nT{\n' + fragment + '\nT}\n.TE'


def source_entries():
    """Actual NODE_LINE gates after graphless words, plus NONEWLINE controls."""
    result = []
    for dialect in ('mdoc', 'man'):
        operands = [('empty', '.No ""' if dialect == 'mdoc' else '.BI "" ""'),
                    ('font', r'.No "\fB"' if dialect == 'mdoc' else r'\fB'),
                    ('continued', r'.No FIRST\c' if dialect == 'mdoc' else r'FIRST\c')]
        for label, operand in operands:
            body = '.nf\n' + operand + '\n' + ('.No SECOND' if dialect == 'mdoc' else 'SECOND') + '\n.fi'
            if dialect == 'mdoc':
                source = document(body, 'DESCRIPTION')
            else:
                source = ('.TH PROBE 1 "November 4, 2026"\n.SH DESCRIPTION\n'
                          + body + '\n.SH NEXT\nAFTER.\n')
            result.append(dict(id=f'source-line-{dialect}-{label}', section='DESCRIPTION',
                               source=source))
    for mode in ('on', 'off'):
        result.append(dict(id=f'source-table-{mode}', section='DESCRIPTION',
                           source=document(f'.Sm {mode}\n.TS\nl.\nCELL\n.TE\n.No A\n.No B',
                                           'DESCRIPTION')))
    return result


def cases():
    result = []
    for section in ('SYNOPSIS', 'DESCRIPTION'):
        for first in ('Fa', 'No'):
            for state, word in [('word', 'arg'), ('bare-zero', r'arg\z'),
                                ('continue', r'arg\c')]:
                for last, operand in [('Ft', 'int'), ('Cd', 'cmd'),
                                      ('In', 'header.h'), ('Fn', 'call'),
                                      ('Nm', 'probe')]:
                    fragment = f'.{first} "{word}"\n.{last} {operand}'
                    result.append(dict(id=f'{section.lower()}-{first}-{state}-{last}',
                                       cohort='core', section=section, fragment=fragment,
                                       source=document(table(fragment), section),
                                       independent_source=document(fragment, section),
                                       contract='recovered'))
    supplements = [
        ('multiword-name', '.No arg\n.Nm prog tail last'),
        ('empty-word', '.Fa ""\n.Ft int'),
        ('cached-glyph', r'.Fa "\zX"' + '\n.Ft int'),
        ('font-only', r'.Fa "\fB"' + '\n.Ft int'),
        ('consecutive-arguments', '.Fa first\n.Fa second\n.Ft int'),
        ('same-line-prefix', '.Pf PRE Fa arg\n.Ft int'),
        ('same-line-flag', '.Fl Fa arg\n.Ft int'),
        ('other-line-prefix', '.Pf PRE\n.Fa arg\n.Ft int'),
        ('other-line-flag', '.Fl\n.Fa arg\n.Ft int'),
        ('transparent-font', r'.Fa "arg\z"' + '\n.ft B\n.Ft int'),
        ('transparent-spacing', r'.Fa "arg\z"' + '\n.Sm on\n.Ft int'),
        ('enclosure', '.Bo\n.Fa "arg\\z"\n.Ft int\n.Bc'),
        ('manual-identity', '.Fa "arg\\z"\n.Ft int\n.Xr printf 3'),
        ('external-identity', '.Fa "arg\\z"\n.Ft int\n.Lk https://example.test LINK'),
        ('author-blank', '.No " "\n.Ft int'),
        ('invisible-cell', '.No \\&\n.Ft int'),
    ]
    for label, fragment in supplements:
        result.append(dict(id='cell-' + label, cohort='risk', section='SYNOPSIS',
                           fragment=fragment, source=document(table(fragment)),
                           independent_source=document(fragment),
                           contract='native-fallback' if label == 'transparent-font' else 'recovered'))
    # These are complete accepted bibliography sequences, separate from the
    # warning recovery inputs. Generated author conjunction/field posts must
    # remain exactly-once under the new source adjacency.
    for spacing in ('on', 'off'):
        for count in (1, 2, 3):
            fields = '\n'.join(f'.%A Author{index}' for index in range(count))
            body = '.Rs\n' + fields + '\n.%T Title\n.%U https://example.test\n.Re'
            mode = '.Sm off\n' if spacing == 'off' else ''
            result.append(dict(id=f'rs-{spacing}-authors-{count}', cohort='bibliography-clean',
                               section='DESCRIPTION', fragment=body,
                               source=document(mode + '.Bl -tag -width Ds\n.It Xo\n' + body
                                               + '\n.Xc\nBODY\n.El', 'DESCRIPTION'),
                               independent_source=document(mode + body, 'DESCRIPTION'),
                               contract='reference-head'))
    for spacing in ('on', 'off'):
        for count in (1, 2, 3):
            fields = '\n'.join(f'.%A A{index}' for index in range(count))
            body = '.Rs\n' + fields + '\n.%T T\n.%U https://e.test\n.Re'
            mode = '.Sm off\n' if spacing == 'off' else ''
            result.append(dict(id=f'rs-root-{spacing}-authors-{count}',
                               cohort='bibliography-clean-root', section='DESCRIPTION',
                               fragment=body,
                               source=document(mode + body + '\n.Sm on\nSECOND', 'DESCRIPTION'),
                               independent_source=document(mode + body, 'DESCRIPTION'),
                               contract='reference-root'))
    for section in ('SYNOPSIS', 'DESCRIPTION'):
        for state, word in [('word', 'arg'), ('bare-zero', r'arg\z'),
                            ('continue', r'arg\c'), ('cached-glyph', r'\zX')]:
            body = f'.Rs\n.Fa "{word}"\n.Ft int\n.Re'
            result.append(dict(id=f'rs-{section.lower()}-{state}',
                               cohort='bibliography-warning', section=section,
                               fragment=body,
                               source=document('.Bl -tag -width Ds\n.It Xo\n' + body
                                               + '\n.Xc\nBODY\n.El', section),
                               independent_source=document(body, section),
                               contract='reference-head'))
    return result
