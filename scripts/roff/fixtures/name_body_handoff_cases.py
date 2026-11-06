"""Finite normal It BODY/Nm sessions; source coordinates are not row events."""

from scripts.roff.fixtures.name_field_session_cases import document, table

STYLES = {
    'tag': '-tag -width 20n', 'hang': '-hang -width 20n',
    'inset': '-inset', 'diag': '-diag', 'ohang': '-ohang', 'item': '-item',
}


def item(style, head='key', name='.Nm probe', body='.Ft int', prefix='', suffix=''):
    content = '\n'.join(part for part in (prefix, '.It' + (' ' + head if head else ''), name, body, suffix)
                        if part)
    return f'.Bl {STYLES[style]}\n{content}\n.El'


def cases():
    output = []
    def add(identifier, cohort, content, *, context='body', head='key'):
        output.append(dict(id=identifier, cohort=cohort, context=context,
                           head=head, source=document(content)))
    for style in ('tag', 'hang', 'inset'):
        add('core-' + style, 'core', item(style))
    for style in ('diag', 'ohang', 'item'):
        add('control-' + style, 'control', item(style, head='' if style == 'item' else 'key'))
    add('control-column', 'control', '.Bl -column abc def\n.It key Ta\n.Nm probe\n.Ft int\n.El')
    add('control-top', 'control', '.Nm probe\n.Ft int', context='top', head='')
    add('control-table', 'control', table('.Nm probe\n.Ft int'), context='table', head='')
    for index, fragment in enumerate(('.Nm\n.Ft int', '.Nm probe\n.Ft int',
                                      '.Nm probe tail', '.Nm probe\n.Ft "int\\z"\n.No NEXT')):
        add(f'control-head-{index}', 'control',
            '.Bl -tag -width 20n\n.It Xo\n' + fragment + '\n.Xc\nBODY\n.El',
            context='head', head='')
        add(f'control-function-{index}', 'control', '.Fo func\n' + fragment + '\n.Fc',
            context='function', head='')
    heads = [('empty', '""'), ('short', 'k'), ('multi', 'key extra'),
             ('overlong', 'longlonglonglonglonglong'), ('invisible', '\\&'),
             ('bare-zero', '\\z'), ('cached-zero', '\\zX'), ('break', 'key\\p'),
             ('continuation', 'key\\c')]
    for index, (label, head) in enumerate(heads):
        style = ('tag', 'hang', 'inset')[index % 3]
        add(f'head-{style}-{label}', 'head', item(style, head=head), head=head)
    bodies = [('empty', ''), ('plain', 'int'), ('argument', '.Fa int'),
              ('external', '.Lk https://example.test LABEL'),
              ('manual', '.Xr printf 3'), ('next-name', '.Ft int\n.Nm next\n.Ar FILE'),
              ('structure', '.Ft int\n.Bl -item -compact\n.It\nBODY\n.El')]
    for index, (label, body) in enumerate(bodies):
        style = ('tag', 'hang', 'inset')[index % 3]
        add(f'body-{style}-{label}', 'body', item(style, body=body))
    for index, (mode, spacing, prefix) in enumerate([
        ('filled', 'on', '.No PREFIX'), ('filled', 'off', '.No PREFIX'),
        ('no-fill', 'on', ''), ('no-fill', 'off', ''),
        ('filled', 'on', '.ft I'), ('no-fill', 'off', '.ft B'),
        ('filled', 'off', '.No ""'), ('no-fill', 'on', '.No \\&'),
        ('filled', 'on', '.No \\zX'), ('no-fill', 'on', '.No X\\c'),
        ('filled', 'off', '.No X\\p'), ('no-fill', 'off', '.No \\z'),
    ]):
        style = ('tag', 'hang', 'inset')[index % 3]
        content = '.Sm ' + spacing + '\n' + ('.nf\n' if mode == 'no-fill' else '')
        content += prefix + '\n' + item(style)
        if mode == 'no-fill':
            content += '\n.fi'
        add(f'state-{index}-{style}-{mode}-{spacing}', 'state', content)
    return output
