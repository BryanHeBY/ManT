"""Canonical finite sources for native word, control and output-owner contracts.

These are exact inputs, including diagnostic recovery controls. Expected output
is recorded separately from the registered pristine mandoc; this module never
runs ManT or computes product expectations.
"""

from pathlib import Path


def native_acceptance_rows():
    """Preserve all 112 original accept/reject sources without rewriting them."""
    root = Path(__file__).resolve().parent.parent
    directory = root / "crates/mant-engine/tests/roff_lowering/shared_execution_matrix/cases"
    sources = []
    for path in sorted(directory.glob("*.1")):
        name = path.stem
        if (name.startswith("a") and len(name) > 1 and name[1].isdigit()) or name in {"man1", "man2"}:
            sources.append({"name": name, "source": path.read_text()})
    return sources


def native_control_rows():
    """The control lies before Tail; it is distinct from a control after Tail."""
    mdoc = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    man = '.TH TEST 1 "September 28, 2026" "Historical Oracle Footer"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n'
    seeds = [('none', ''), ('empty', '.No ""\n'), ('bare_zero', '.No "\\z"\n'), ('pending_zero', '.No "\\zX"\n'), ('invisible', '.No "\\&"\n'), ('rejected', '.No "\\p X"\n'), ('partial', '.No X\n.No "\\p"\n'), ('embedded', '.No "X\\p Y"\n')]
    controls = [('none', ''), ('font', '.ft B\n'), ('tabs', '.ta 4n 8n\n'), ('target', '.Tg marker\n'), ('break', '.br\n'), ('space_zero', '.sp 0\n'), ('space_one', '.sp 1\n'), ('space_negative', '.sp -1\n'), ('margin', '.mc\n'), ('paragraph', '.Pp\n'), ('no_fill', '.nf\n'), ('indent', '.ti 2n\n')]
    exits = [('fill', '.fi\n'), ('two_margin', '.mc\n.mc\n'), ('two_break', '.br\n.br\n'), ('two_space', '.sp 1\n.sp 2\n'), ('paragraph', '.Pp\n'), ('section', None), ('eof', None)]

    def dialectize(text, dialect):
        if dialect == 'mdoc':
            return text
        return text.replace('.No ""\n', '.B ""\n').replace('.No ', '').replace('.Pp\n', '.PP\n').replace('.Tg marker\n', '.ft R\n')
    cases = []
    for dialect, header, tail, nextsec in [('mdoc', mdoc, '.No Tail\n.br\n.No After\n', '.Sh NEXT\n.No END\n'), ('man', man, 'Tail\n.br\nAfter\n', '.SH NEXT\nEND\n')]:
        for seed, source in seeds:
            for control, request in controls:
                cases.append(dict(name=f'{dialect}_word_{seed}_control_{control}', source=header + dialectize(source + request, dialect) + tail + nextsec, eof=False))
        for seed in ['pending_zero', 'rejected', 'embedded']:
            source = next((source for name, source in seeds if name == seed))
            for exit, request in exits:
                suffix = nextsec if exit == 'section' else '' if exit == 'eof' else dialectize(request, dialect) + tail + nextsec
                cases.append(dict(name=f'{dialect}_word_{seed}_exit_{exit}', source=header + dialectize(source, dialect) + suffix, eof=exit == 'eof'))
    return cases

def generated_word_rows():
    """Diag uses a literal It HEAD. Line-start Ns is its explicit no-op control."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    footer = '.No End\n.Sh NEXT\n.No END\n'
    pending = ['none', 'bare_zero', 'cached', 'cached_held', 'rearmed', 'continued']
    held = [('none', ''), ('space', ' '), ('two_spaces', '  '), ('three_spaces', '   '), ('tab', '\t'), ('space_tab', ' \t'), ('nonbreaking', '\\~'), ('digit_space', '\\0'), ('invisible', '\\&'), ('word_end', '\\p')]
    nexts = [('plain', 'No B'), ('strong', 'Sy B'), ('emphasis', 'Em B'), ('glyph', 'No \\(bu'), ('square', 'Bq B'), ('round', 'Pq B'), ('optional', 'Op B'), ('quoted', 'Dq B'), ('flag', 'Fl x'), ('include', 'In x.h'), ('bsd', 'Bx'), ('inset', None), ('diagnostic', None)]
    boundaries = ['tight', 'word', 'spacing_off']

    def seed(state, blank):
        if state == 'none':
            return blank
        if state == 'bare_zero':
            return '\\z' + blank
        if state == 'cached':
            return '\\zA' + blank
        if state == 'cached_held':
            return '\\zA ' + blank
        if state == 'rearmed':
            return '\\zA' + blank + '\\z'
        return '\\zA' + blank + '\\c'
    cases = []
    for state in pending:
        for blankname, blank in held:
            for nextname, entry in nexts:
                for boundary in boundaries:
                    prefix = '.Sm off\n' if boundary == 'spacing_off' else ''
                    text = seed(state, blank)
                    if entry is None:
                        kind = 'inset' if nextname == 'inset' else 'diag'
                        no_space = '.Ns\n' if boundary == 'tight' else ''
                        body = f'{prefix}.Bl -{kind} -compact\n.It "{text}"\n{no_space}.No B\n.El\n'
                    else:
                        join = ' Ns' if boundary == 'tight' else ''
                        body = f'{prefix}.No "{text}"{join} {entry}\n'
                    cases.append(dict(name=f'{state}_{blankname}_{nextname}_{boundary}', source=header + body + footer))
    return cases

def generated_body_rows():
    """Macro expansions create two NODE_LINE events at identical coordinates."""
    mdoc = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    man = '.TH TEST 1 "September 28, 2026" "Historical Oracle Footer"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n'
    next_mdoc = '.Sh NEXT\n.No END\n'
    next_man = '.SH NEXT\nEND\n'
    cases = []
    scopes = [('UR', 'UE', 'https://example.org'), ('MT', 'ME', 'user@example.org')]
    states = [('word_end', 'prefix\\p\n', ''), ('bare_zero', '\\z\n', ''), ('same_coordinate', '', '.de XX\nfirst\nsecond\n..\n')]
    bodies = [('plain', 'label\n'), ('bold', '.B label\n'), ('empty', ''), ('invisible', '\\&\n')]
    for opening, closing, target in scopes:
        for bodyname, body in bodies:
            for targetname, address in [('normal', target), ('empty', '\\&')]:
                for state, prefix, definition in states:
                    expanded = '.XX\n' if state == 'same_coordinate' else ''
                    source = man + definition + '.nf\n' + prefix + f'.{opening} {address}\n' + expanded + body + f'.{closing}\nafter\n.fi\n' + next_man
                    cases.append(dict(name=f'link_{opening}_{bodyname}_{targetname}_{state}', source=source, same_coordinate=state == 'same_coordinate'))
    structures = [('paragraph', '.PP\nbody\n'), ('item', '.IP item 4\nbody\n'), ('relative', '.RS 4\nbody\n.RE\n'), ('fill', '.fi\nbody\n.nf\nsecond\n'), ('nested', '.UR https://inner.example.org\ninner\n.UE\n')]
    for opening, closing, target in scopes:
        for bodyname, body in structures:
            for state, prefix, definition in states:
                expanded = '.XX\n' if state == 'same_coordinate' else ''
                source = man + definition + '.nf\n' + prefix + f'.{opening} {target}\n' + expanded + 'first\n' + body + f'.{closing}\nafter\n.fi\n' + next_man
                cases.append(dict(name=f'link_{opening}_{bodyname}_{state}', source=source, same_coordinate=state == 'same_coordinate'))
    for opening, closing, target in scopes:
        for where in ['before', 'tail']:
            for continued in [False, True]:
                operand = '\\zX' + ('\\c' if continued else '') + '\n'
                prefix = operand if where == 'before' else ''
                body = 'label\n' + (operand if where == 'tail' else '')
                source = man + '.nf\n' + prefix + f'.{opening} {target}\n' + body + f'.{closing}\nafter\n.fi\n' + next_man
                cases.append(dict(name=f"link_{opening}_{where}_zero_{('continued' if continued else 'ordinary')}", source=source, same_coordinate=False))
    contexts = ['no_fill', 'literal', 'one_line', 'literal_line']
    macros = [('angle', '.Ao\n', '.Ac\n'), ('square', '.Bo\n', '.Bc\n'), ('function', '.Fo call\n', '.Fc\n'), ('author', '', '')]
    for context in contexts:
        for macro, open_scope, close_scope in macros:
            for state in ['word_end', 'bare_zero', 'same_coordinate']:
                carrier = {'no_fill': 'No', 'literal': 'No', 'one_line': 'D1', 'literal_line': 'Dl'}[context]
                if macro == 'author':
                    carrier += ' An'
                if state == 'same_coordinate':
                    definition = f'.de XX\n.{carrier} first\n.{carrier} second\n..\n'
                    content = '.XX\n'
                else:
                    definition = ''
                    operand = 'X\\p' if state == 'word_end' else '\\z'
                    content = f'.{carrier} {operand}\n'
                wrapper = '.Bd -literal -compact\n' if context == 'literal' else ''
                wrapper_end = '.Ed\n' if context == 'literal' else ''
                source = mdoc + definition + '.nf\n' + wrapper + open_scope + content + close_scope + wrapper_end + '.No after\n.fi\n' + next_mdoc
                cases.append(dict(name=f'mdoc_{context}_{macro}_{state}', source=source, same_coordinate=state == 'same_coordinate'))
    return cases

def kept_word_rows():
    """Leading formatter blanks remain gold; KEEP is not inferred from glyphs."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    footer = '.br\n.No End\n.Sh NEXT\n.No END\n'
    words = [('plain', 'No B'), ('empty', 'No ""'), ('invisible', 'No \\&'), ('generated', 'Bq B')]
    boundaries = ['word', 'tight', 'spacing_off', 'word_end', 'continued_break', 'pending_zero']
    cases = []
    for keep in [False, True]:
        for word, entry in words:
            for line in ['same', 'cross']:
                for boundary in boundaries:
                    prefix = 'A' + {'word_end': '\\p', 'continued_break': '\\p\\c', 'pending_zero': '\\zX'}.get(boundary, '')
                    mode = '.Sm off\n' if boundary == 'spacing_off' else ''
                    tight = ' Ns' if boundary == 'tight' else ''
                    if line == 'same':
                        body = f'{mode}.No {prefix}{tight} {entry} No Last\n'
                    else:
                        body = f'{mode}.No {prefix}{tight}\n.{entry} No Last\n'
                    source = header + ('.Bk -words\n' if keep else '') + body + ('.Ek\n' if keep else '') + ('.Sm on\n' if boundary == 'spacing_off' else '') + footer
                    cases.append(dict(name=f"{('kept' if keep else 'ordinary')}_{word}_{line}_{boundary}", source=source))
    return cases

def generated_word_styles():
    """Only the accepted prior A keeps its original font and semantic owner."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    footer = '.No End\n.Sh NEXT\n.No END\n'
    cases = []
    for state, seed in [('no_held', '\\zA'), ('held_rearmed', '\\zA \\z')]:
        for style in ['No', 'Sy', 'Em', 'Li']:
            for nextname, entry in [('mail', 'Mt user@example.org'), ('link', 'Lk https://example.org B')]:
                for boundary in ['tight', 'word', 'spacing_off']:
                    mode = '.Sm off\n' if boundary == 'spacing_off' else ''
                    tight = ' Ns' if boundary == 'tight' else ''
                    cases.append(dict(name=f'{state}_{style}_{nextname}_{boundary}', source=header + mode + f'.{style} "{seed}"{tight} {entry}\n' + footer))
    return cases

def output_owner_rows():
    """Authored Tg targets use the private-marker-like name deliberately."""
    mdoc = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    man = '.TH TEST 1 "September 28, 2026" "Historical Oracle Footer"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n'
    footer = '.Sh NEXT\n.No END\n'
    cases = []
    payloads = [('word', '.No Word\n'), ('two_words', '.No Word NextWord\n'), ('target', '.Tg mant-field-word-2-0\n.No Word\n'), ('link', '.Lk https://example.org LABEL\n'), ('rejected', '.No X\\p\n.No "\\p DROP"\n.No Tail\n')]
    sinks = ['paragraph', 'no_fill', 'literal', 'one_line', 'literal_line', 'head', 'body', 'table', 'heading']
    for sink in sinks:
        for payload, body in payloads:
            reference_fragment = None
            if sink == 'paragraph':
                wrapped = body
            elif sink == 'no_fill':
                wrapped = '.nf\n' + body + '.fi\n'
            elif sink == 'literal':
                wrapped = '.Bd -literal -compact\n' + body + '.Ed\n'
            elif sink == 'one_line':
                wrapped = body.replace('.No ', '.D1 ').replace('.Lk ', '.D1 Lk ')
            elif sink == 'literal_line':
                wrapped = body.replace('.No ', '.Dl ').replace('.Lk ', '.Dl Lk ')
            elif sink == 'head':
                wrapped = '.Bl -tag -width 20n -compact\n.It Xo\n' + body + '.Xc\n.No BodyWord\n.El\n'
            elif sink == 'body':
                wrapped = '.Bl -tag -width 20n -compact\n.It HeadWord\n' + body + '.El\n'
            elif sink == 'table':
                if payload == 'link':
                    body = '.Lk https://example.org\n'
                if payload == 'target':
                    body = '.No Word\n'
                # The normal tbl profile ignores these macro names. The
                # frozen recovery contract separately executes this exact
                # T{} contents as mdoc; neither source replaces the other.
                reference_fragment = mdoc + body + footer
                wrapped = ('.Tg mant-field-word-2-0\n' if payload == 'target' else '') + '.TS\nl.\nT{\n' + body + 'T}\n.TE\n'
            elif payload == 'target':
                wrapped = '.Tg mant-field-word-2-0\n.Ss Word\n.No BodyWord\n'
            elif payload == 'link':
                wrapped = '.Ss Lk https://example.org LABEL\n.No BodyWord\n'
            elif payload == 'rejected':
                wrapped = '.Ss No "X\\p \\p DROP"\n.No BodyWord\n'
            else:
                wrapped = body.replace('.No ', '.Ss ') + '.No BodyWord\n'
            case = dict(name=f'sink_{sink}_{payload}', source=mdoc + wrapped + footer, target=payload == 'target', links=1 if payload == 'link' else 0, first_label=None)
            if reference_fragment is not None:
                case.update(scope='table-macro-recovery', reference_fragment=reference_fragment)
            cases.append(case)
    for opening, closing, address in [('UR', 'UE', 'https://example.org'), ('MT', 'ME', 'user@example.org')]:
        for prefixname, prefix in [('none', ''), ('word', 'prefix\n'), ('styled', '.B prefix\n.I words\n')]:
            for drain, body in [('none', 'first\n'), ('paragraph', 'first\n.PP\nbody\n'), ('item', 'first\n.IP item 4\nbody\n'), ('fill', 'first\n.nf\nbody\n.fi\n')]:
                source = man + prefix + f'.{opening} {address}\n' + body + f'.{closing}\nafter\n.SH NEXT\nEND\n'
                cases.append(dict(name=f'link_{opening}_{prefixname}_{drain}', source=source, target=False, links=1, first_label='first'))
    return cases

def portable_word_rows():
    """Native gold is independent of the selected portable Bx display spelling."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    footer = '.Sh NEXT\n.No END\n'
    operands = [('alpha', '-alpha\\p'), ('beta', '-beta\\c'), ('development', '-devel\\zX'), ('word_end', '\\p'), ('bare_zero', '\\z'), ('pending_zero', '\\zX'), ('continued_zero', '\\z\\c'), ('font', '\\fB'), ('version_release', '4.4 Lite')]
    contexts = ['paragraph', 'head', 'body', 'no_fill']
    surroundings = ['plain', 'typed', 'partial']
    cases = []
    for operandname, operand in operands:
        for context in contexts:
            for surrounding in surroundings:
                if surrounding == 'typed':
                    content = '.Lk https://before.example BEFORE\n.Tg mant-field-word-2-0\n.Bf -emphasis\n' + f'.Bx {operand}\n' + '.Ef\n.Lk https://after.example AFTER\n'
                elif surrounding == 'partial':
                    content = '.No X\\p\n' + f'.Bx {operand}\n' + '.No "\\p DROP"\n.br\n.No After\n'
                else:
                    content = '.No Before\n' + f'.Bx {operand}\n' + '.No After\n'
                if context == 'head':
                    content = '.Bl -inset -compact\n.It Xo\n' + content + '.Xc\n.No BodyWord\n.El\n'
                elif context == 'body':
                    content = '.Bl -inset -compact\n.It HeadWord\n' + content + '.El\n'
                elif context == 'no_fill':
                    content = '.nf\n' + content + '.fi\n'
                # This family checks accepted native hard rows and portable
                # display invariance, not width-dependent device soft wraps.
                # Preserve the ordinary five-profile record and additionally
                # record the same source at CVS's maximum accepted width
                # (manpath.c::manconf_output(), strtonum(..., 1, 1000, ...)).
                cases.append(dict(name=f'{operandname}_{context}_{surrounding}', source=header + content + footer,
                                  scope='responsive-hard-rows', wide_width=1000))
    return cases

def container_word_rows():
    """TBL uses raw text cells; alternate man operands use pre_alternate."""
    mdoc = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    man = '.TH TEST 1 "September 28, 2026" "Historical Oracle Footer"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n'
    footer = '.Sh NEXT\n.No END\n'
    cases = []
    seeds = [('accepted', '.No "X\\p Y"\n.No Tail\n'), ('rejected', '.No "\\p DROP"\n.No Tail\n'), ('pending', '.No "\\zX"\n.No Tail\n')]
    for context in ['nested_list', 'table', 'equation', 'display_scope']:
        for state, body in seeds:
            if context == 'nested_list':
                content = '.Bl -inset -compact\n.It Outer\n.Bl -inset -compact\n.It Inner\n' + body + '.El\n.El\n'
            elif context == 'table':
                cell = body.replace('.No ', '').replace(chr(34), '')
                content = '.TS\nl.\nT{\n' + cell + 'T}\n.TE\n'
            elif context == 'equation':
                content = body + '.EQ\nx\n.EN\n.No Following\n'
            else:
                content = '.Ao\n.Bd -literal -compact\n' + body + '.Ac\n.No Following\n.Ed\n.No Final\n'
            case = dict(name=f'{context}_{state}', source=mdoc + content + footer)
            if context == 'equation':
                # Public block equation layout deliberately does not claim
                # the term_eqn inline-device geometry or flush behavior.
                case['scope'] = 'display-equation'
            cases.append(case)
    for macro in ['B', 'BI', 'BR']:
        for state, operand in [('accepted', 'X\\p Y'), ('rejected', '\\p DROP'), ('pending', '\\zX')]:
            content = f'.{macro} "{operand}" "Next"\nTail\n.br\nAfter\n'
            cases.append(dict(name=f'man_{macro}_{state}', source=man + content + '.SH NEXT\nEND\n'))
    return cases

def matrices():
    """Return independent source families in stable recording order."""
    return [
        ("native_acceptance_rows", native_acceptance_rows()),
        ("native_control_rows", native_control_rows()),
        ("generated_word_rows", generated_word_rows()),
        ("generated_body_rows", generated_body_rows()),
        ("kept_word_rows", kept_word_rows()),
        ("generated_word_styles", generated_word_styles()),
        ("output_owner_rows", output_owner_rows()),
        ("portable_word_rows", portable_word_rows()),
        ("container_word_rows", container_word_rows()),
    ]


COUNTS = {'native_acceptance_rows': 112, 'native_control_rows': 234, 'generated_word_rows': 2340, 'generated_body_rows': 134, 'kept_word_rows': 96, 'generated_word_styles': 48, 'output_owner_rows': 69, 'portable_word_rows': 108, 'container_word_rows': 21}
