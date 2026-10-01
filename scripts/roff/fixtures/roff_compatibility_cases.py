"""Canonical finite sources for native word, control and output-owner contracts.

These are exact inputs, including diagnostic recovery controls. Expected output
is recorded separately from the registered pristine mandoc; this module never
runs ManT or computes product expectations.
"""

from itertools import product
from pathlib import Path


def native_acceptance_rows():
    """Preserve all 112 original accept/reject sources without rewriting them."""
    root = Path(__file__).resolve().parents[3]
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

def node_body_rows():
    """Real HEAD fitting and first BODY events, including section/EOF edges."""
    man = '.TH TEST 1 "September 28, 2026"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n'
    mdoc = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    states = [('empty-text', '.B ""'), ('zero-width', '\\&'), ('armed-zero', '\\z')]
    cases = []
    for macro, width, head, (state, text), control in product(
            ['TP', 'TQ', 'IP'], [4, 8, 16], ['Short', 'HeadWord'],
            states, ['.br', '.sp 1', '.PP']):
        request = (f'.IP {head} {width}\n' if macro == 'IP'
                   else f'.{macro} {width}\n{head}\n')
        cases.append(dict(
            name=f'{macro.lower()}-{width}-{head.lower()}-{state}-{control[1:].replace(" ", "-")}',
            source=man + request + text + '\n' + control + '\nAFTER\n.SH NEXT\nEND\n',
            heading='NEXT', macro=macro, head=head))
    for state, text in states[:2]:
        for boundary, suffix in [('eof-paragraph', '.PP\n'),
                                 ('section', '.SH EXITMARK\nEXITBODY\n')]:
            cases.append(dict(
                name=f'tp-headword-{state}-{boundary}',
                source=man + '.TP 8\nHeadWord\n' + text + '\n' + suffix,
                heading=None if boundary == 'eof-paragraph' else 'EXITMARK',
                macro='TP', head='HeadWord'))
    for kind, carrier, control in product(['tag', 'hang'], ['No', 'Em', 'Lk'], ['br', 'Pp']):
        operand = ('https://ex.org ' if carrier == 'Lk' else '') + '"D \\p E"'
        cases.append(dict(
            name=f'{kind}-{carrier.lower()}-{control.lower()}',
            source=mdoc + f'.Bl -{kind} -width 8n\n.It HeadWord\n.{carrier} {operand}\n'
                   + f'.{control}\n.No AFTER\n.El\n.Sh NEXT\n.No END\n',
            heading='NEXT', macro='It', head='HeadWord'))
    # A zero-width accepted prefix and a rejected visible suffix are
    # different from a visible accepted description. Both must report
    # boundary order from actual word events, never the final wrapper.
    labels = [
        ('zero-rejected', '\\p\\& \\p Z'),
        ('zero-visible', '\\p\\& Z'),
        ('zero-marker', '\\p\\& \\p'),
        ('zero-only', '\\p\\&'),
        ('rejected', '\\p  Z'),
        ('prefix-rejected', 'D\\p \\p Z'),
        ('delayed-rejected', '\\p\\zY Z'),
    ]
    for kind, carrier, (label_name, label), control in product(
            ['tag', 'hang'], ['No', 'Em', 'Lk'], labels, ['br', 'Pp']):
        operand = ('https://ex.org ' if carrier == 'Lk' else '') + f'"{label}"'
        cases.append(dict(
            name=f'{kind}-{carrier.lower()}-{label_name}-{control.lower()}',
            source=mdoc + f'.Bl -{kind} -width 8n\n.It HeadWord\n.{carrier} {operand}\n'
                   + f'.{control}\n.No AFTER\n.El\n.Sh NEXT\n.No END\n',
            heading='NEXT', macro='It', head='HeadWord'))
    for width, head, carrier, (label_name, label), control in product(
            [4, 16], ['Short', 'HeadWord'], ['No', 'Em', 'Lk'],
            [('accepted-zero-leading', '\\&\\p \\p Y'),
             ('accepted-zero-marker', '\\p\\& \\p Y')], ['none', 'Pp']):
        operand = ('https://ex.org ' if carrier == 'Lk' else '') + f'"{label}"'
        request = '' if control == 'none' else '.Pp\n'
        cases.append(dict(
            name=f'tag-{width}-{head.lower()}-{carrier.lower()}-{label_name}-{control.lower()}',
            source=mdoc + f'.Bl -tag -width {width}n\n.It {head}\n.{carrier} {operand}\n'
                   + request + '.No AFTER\n.El\n.Sh NEXT\n.No END\n',
            heading='NEXT', macro='It', head=head))
    return cases


def section_edge_rows():
    """Executed blank HEAD rows and retained paragraphs at section/EOF edges."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    cases = []
    prefixes = [('none', ''), ('blank', '\n'), ('zero-width', '.No \\&\n'),
                ('empty-word', '.No ""\n')]
    suffixes = [('eof', ''), ('section', '.Sh NEXT\n.No END\n'),
                ('body', '.No BodyWord\n.Sh NEXT\n.No END\n')]
    for kind, no_fill, (prefix_name, prefix), request, (tail, suffix) in product(
            ['tag', 'hang', 'ohang', 'inset'], [False, True], prefixes,
            ['.sp 1', '.sp 2', '.Pp'], suffixes):
        # The BODY source stays outside Xo. Its AST ownership is asserted
        # by the regression test, including diagnostic recovery inputs.
        before = '.nf\n' if no_fill else ''
        body = '.No BodyWord\n' if tail == 'body' else ''
        after = '.Sh NEXT\n.No END\n' if tail != 'eof' else ''
        source = (header + before + f'.Bl -{kind} -width 8n\n.It Xo\n'
                  + prefix + request + '\n.Xc\n' + body + '.El\n' + after)
        cases.append(dict(
            name=f'{kind}-{("literal" if no_fill else "filled")}-{prefix_name}-{request[1:].replace(" ", "-")}-{tail}',
            source=source, heading=None if tail == 'eof' else 'NEXT',
            owner='It', body_word=tail == 'body'))
    for prefix_name, prefix in [('blank-paragraph', '\n.Pp\n'),
                                ('paragraph', '.Pp\n'), ('blank', '\n')]:
        for tail, suffix in [('eof', ''), ('section', '.Sh NEXT\n.No END\n'),
                             ('word', '.No AFTER\n.Sh NEXT\n.No END\n')]:
            cases.append(dict(name=f'section-{prefix_name}-{tail}',
                              source=header + prefix + suffix,
                              heading=None if tail == 'eof' else 'NEXT',
                              owner='Sh', body_word=False))
    return cases


def field_spacing_rows():
    """Skip-vspace debt cannot commit tentative field padding before printing."""
    from scripts.roff.fixtures.roff_acceptance_cases import spacing_interval_cases
    return [dict(name=f'hang-{case["width"]}-{index}', source=case['source'],
                 heading='NEXT', macro='It', owner='Xo',
                 wide_width=1000 if case['body_carrier'] == 'lk' else None)
            for index, case in enumerate(spacing_interval_cases())
            if case['kind'] == 'hang' and case['control'] == 'spneg1-sp1']


def empty_text_continuation_rows():
    """Empty source TEXT calls the same conditional native newline as scopes."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    cases = []
    for no_fill, (kind, width) in product([False, True],
                                        [('hang', 4), ('hang', 8), ('tag', 4), ('tag', 8)]):
        source = (header + ('.nf\n' if no_fill else '')
                  + f'.Bl -{kind} -width {width}n\n.It Xo\n.No X\\c\n\n.No Y\n'
                  + '.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n')
        cases.append(dict(name=f'{kind}-{width}-{("literal" if no_fill else "filled")}',
                          source=source, heading='NEXT', macro='It', owner='Xo'))
    return cases


def plain_field_rows():
    """Ordinary flush units retain accepted invisible passes and reject tails."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    payloads = [r'\& \p Y', r'\&\p \p Y', r'\p\& \p Y', r'\p',
                r'\&', r'\[u0301]\p Y', r'\z', r'\zX']
    cases = []
    for no_fill, prefix, (index, payload) in product(
            [False, True], [False, True], enumerate(payloads)):
        source = (header + ('.nf\n' if no_fill else '')
                  + ('.No BEFORE\n' if prefix else '')
                  + f'.No "{payload}"\n.No AFTER\n'
                  + ('.fi\n' if no_fill else '') + '.Sh NEXT\n.No END\n')
        cases.append(dict(
            name=f'{("literal" if no_fill else "filled")}-{("prefix" if prefix else "origin")}-{index}',
            source=source, no_fill=no_fill, prefix=prefix, payload=payload,
            heading='NEXT'))
    return cases


def literal_eof_rows():
    """Physical literal rows survive at EOF without a following printed word."""
    headers = {
        'man': '.TH TEST 1 "September 28, 2026" "Historical Oracle Footer"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n',
        'mdoc': '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n',
    }
    cases = []
    for dialect, prefix, payload, repetitions, ending in product(
            headers, [False, True], ['blank', 'zero-width', 'empty-text'],
            [1, 2], ['eof', 'fill', 'break-fill']):
        word = '.B' if dialect == 'man' else '.No'
        line = {'blank': '\n', 'zero-width': '\\&\n',
                'empty-text': f'{word} ""\n'}[payload]
        suffix = {'eof': '', 'fill': '.fi\n', 'break-fill': '.br\n.fi\n'}[ending]
        cases.append(dict(
            name=f'{dialect}-{("prefix" if prefix else "origin")}-{payload}-{repetitions}-{ending}',
            source=headers[dialect] + '.nf\n' + ('BEFORE\n' if prefix else '')
                   + line * repetitions + suffix,
            heading=None, dialect=dialect, payload=payload))
    return cases


def table_control_rows():
    """Real list controls cannot introduce a post into the live column row."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    cases = []
    for width, carrier, (before, after) in product(
            [4, 8, 10, 16], ['No', 'Em', 'Lk https://ex.org'],
            [(False, False), (True, False), (False, True), (True, True)]):
        source = (header + f'.Bl -column "{"x" * width}" "b"\n.It A\n'
                  + '.Bl -tag -compact -width 2n\n'
                  + ('.ft B\n' if before else '') + f'.It {carrier} x\n.No B\n'
                  + ('.ft R\n' if after else '')
                  + '.El\n.Ta C\n.El\n\n.Sh NEXT\n.No END\n')
        cases.append(dict(
            name=f'{width}-{carrier.split()[0]}-{int(before)}-{int(after)}',
            source=source, heading='NEXT', carrier=carrier, width=width,
            before=before, after=after))
    return cases


def skipped_list_heads():
    """Plain list It pre declines authored HEAD children in every output mode."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    payloads = [('break', '.No "\\p HEAD"\n'),
                ('zero', '.No HEAD\\zX\\c\n'),
                ('font', '.ft I\n'),
                ('link', '.Lk https://head.invalid HEAD\n')]
    controls = [('none', ''), ('break', '.br\n'), ('space', '.sp 1\n'),
                ('literal', '.nf\n')]
    cases = []
    for kind, no_fill, (payload, head), (control, request) in product(
            ['item', 'bullet', 'dash', 'enum'], [False, True], payloads, controls):
        prefix = header + ('.nf\n' if no_fill else '') + f'.Bl -{kind} -width 4n\n.It Xo\n'
        suffix = '.Xc\n' + request + '.No BodyWord\n.No TailWord\n.El\n.fi\n.Sh NEXT\n.No END\n'
        cases.append(dict(
            name=f'{kind}-{("literal" if no_fill else "filled")}-{payload}-{control}',
            source=prefix + head + suffix, baseline_source=prefix + '.No ""\n' + suffix,
            heading='NEXT', kind=kind, payload=payload, control=control,
            reading_rule='skipped-head-execution-invariance'))
    return cases


def structural_row_handoffs():
    """Bl BLOCK pre consumes the previous HEAD device row before a nested It."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    cases = []
    for style, width, no_fill, continuation in product(
            ['tag', 'hang'], [4, 16], [False, True], [False, True]):
        source = (header + ('.nf\n' if no_fill else '')
                  + f'.Bl -{style} -width {width}n\n.It Xo\n.No X'
                  + ('\\c' if continuation else '') + '\n.Xc\n'
                  + '.Bl -tag -width inner\n.It INNER\n.No BodyWord\n.El\n'
                  + '.No AFTER\n.El\n.fi\n.Sh NEXT\n.No END\n')
        cases.append(dict(
            name=f'{style}-{width}-{("literal" if no_fill else "filled")}-{int(continuation)}',
            source=source, heading='NEXT', style=style, width=width,
            no_fill=no_fill, continuation=continuation))
    return cases


def column_margin_rows():
    """Deferred column padding stays with its resumed source word."""
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
    cases = []
    for carrier, offset, (name, operand) in product(
            ['No', 'Em', 'Sy', 'Li', 'Lk'], [0, 6],
            [('accepted', r'X\p Y\c'), ('rejected', r'X\p \p DROP\c')]):
        target = 'https://ex.org ' if carrier == 'Lk' else ''
        source = (header + '.Bl -column "xxxxxxxx" "xxxx"\n.It Xo\n'
                  + f'.Bd -literal -compact -offset {offset}n\n.{carrier} {target}"{operand}"\n'
                  + '.mc\n.Ed\n.No AFTER\n.Xc Ta RightWord\n.El\n.Sh NEXT\n.No END\n')
        cases.append(dict(name=f'{carrier.lower()}-{offset}-{name}', source=source,
                          heading='NEXT', carrier=carrier, offset=offset))
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
        ("node_body_rows", node_body_rows()),
        ("section_edge_rows", section_edge_rows()),
        ("field_spacing_rows", field_spacing_rows()),
        ("empty_text_continuation_rows", empty_text_continuation_rows()),
        ("plain_field_rows", plain_field_rows()),
        ("literal_eof_rows", literal_eof_rows()),
        ("column_margin_rows", column_margin_rows()),
        ("table_control_rows", table_control_rows()),
        ("skipped_list_heads", skipped_list_heads()),
        ("structural_row_handoffs", structural_row_handoffs()),
    ]


COUNTS = {'native_acceptance_rows': 112, 'native_control_rows': 234, 'generated_word_rows': 2340, 'generated_body_rows': 134, 'kept_word_rows': 96, 'generated_word_styles': 48, 'output_owner_rows': 69, 'portable_word_rows': 108, 'container_word_rows': 21, 'node_body_rows': 262, 'section_edge_rows': 297}
COUNTS.update(node_body_rows=310, field_spacing_rows=72, empty_text_continuation_rows=8)
COUNTS.update(plain_field_rows=32)
COUNTS.update(literal_eof_rows=72)
COUNTS.update(column_margin_rows=20)
COUNTS.update(table_control_rows=48)
COUNTS.update(skipped_list_heads=128)
COUNTS.update(structural_row_handoffs=16)
