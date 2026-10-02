"""Retained source generators for independently reviewed grammar/control rules.

The original diagnosed diag extended-HEAD templates remain recovery evidence. New
legal family coverage lives in rule_closure_fields; a retained identity is
never evidence that its declared HEAD state was reachable.
"""

import itertools

HEAD = '.Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n'
TAIL = '.Sh NEXT\n.No END\n'

def grammar():
    samples = {}
    for trig in 'FMkmYf':
        for name, arg in [('single', 'B'), ('paren', '(BI'), ('bracket', '[BI]'), ('empty', '[]'), ('blank', '[ B]'), ('missing', ''), ('partial', '[BI')]:
            samples[f'{trig}-{name}'] = '\\' + trig + arg
    samples.update({'O-valid': '\\O1', 'O-bracket': '\\O[2]', 'O-invalid': '\\O[abc]'})
    for arg in ["'12'", "+'12'", "-'12'", '[12]', '(12', '12', '+12', '-12', "'", '[', '(', "'1\\&2'", '\\&2']:
        samples['size-' + arg] = '\\s' + arg
    samples.update({'named-paren': '\\(aq', 'named-bracket': '\\[aq]', 'zero': '\\&', 'zero-graph': '\\:', 'skip': '\\z', 'break': '\\p', 'concat': '\\c', 'device': '\\*[.T]', 'unknown': '\\q', 'copy-font': '\\EfB', 'copy-size': "\\Es'12'", 'font-nested': '\\f\\&B', 'color-nested': '\\m\\&B', 'quoted-nested': "\\X'Q'", 'quoted-escaped': '\\X\\(aqQ\\(aq'})
    for (name, inner), outer, delim, carrier in itertools.product(samples.items(), ['o', 'X', 'Z'], ["'", '|', '\\(aq'], ['No', 'Lk']):
        body = f'.{carrier}' + (' https://ex.org' if carrier == 'Lk' else '') + ' "A\\' + outer + delim + 'X' + inner + 'Y' + delim + 'Z"\n.No AFTER\n'
        yield dict(family='grammar', inner_name=name, inner=inner, outer=outer, delimiter=delim, carrier=carrier, source=HEAD + body + TAIL)
    for (name, inner), outer in itertools.product(samples.items(), ['o', 'X', 'Z']):
        s = '.TH TEST 1 "October 2, 2026"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\nA\\' + outer + '|X' + inner + 'Y|Z\nAFTER\n.SH NEXT\nEND\n'
        yield dict(family='grammar-man', inner_name=name, inner=inner, outer=outer, source=s)
CONTROLS = {'none': '', 'nf-fi': '.nf\n.fi\n', 'nf-br-fi': '.nf\n.br\n.fi\n', 'br-nf-fi': '.br\n.nf\n.fi\n', 'nf-fi-br': '.nf\n.fi\n.br\n', 'nf-ft-fi': '.nf\n.ft B\n.fi\n', 'nf-word-fi': '.nf\n.No MID\n.fi\n', 'nf-zero-fi': '.nf\n.No "\\&"\n.fi\n', 'nf-sp-fi': '.nf\n.sp 1\n.fi\n', 'nf-neg-fi': '.nf\n.sp -1\n.fi\n', 'repeat': '.nf\n.fi\n.nf\n.fi\n', 'mc-nf-fi': '.mc |\n.nf\n.fi\n.mc\n', 'nf-mc-fi': '.nf\n.mc |\n.fi\n.mc\n', 'ti-nf-fi': '.ti 2n\n.nf\n.fi\n', 'br-ft-br': '.br\n.ft I\n.br\n', 'sp-neg-sp': '.sp 1\n.sp -1\n.sp 1\n'}

def fields():
    for kind, label, (request, control) in itertools.product(['tag', 'hang', 'inset', 'diag', 'ohang', 'column'], [None, '', '\\&', '\\:', '\\z', '\\zX', '\\0', 'A', 'A\\p', '\\&\\p', ' ', 'A\\c'], CONTROLS.items()):
        start = f'.Bl -{kind} -width 8n\n.It Xo\n' if kind != 'column' else '.Bl -column "xxxxxxxx" "xxxx"\n.It Xo\n'
        word = '' if label is None else '.No "' + label + '"\n'
        end = '.Xc\n.No BodyWord\n.El\n' if kind != 'column' else '.Xc Ta RIGHT\n.El\n'
        s = HEAD + start + word + control + '.No AFTER\n' + end + TAIL
        yield dict(family='field-transition', kind=kind, label=label, request=request, source=s)

def boundaries():
    for outer, delim, payload, end in itertools.product(['o', 'X', 'Z', 'C', 'N', 'h'], ["'", '|', '\\(aq', '\\m[red]', '\\fB'], ['', 'X', '\\s\\&2', '\\f[ B]', "\\s'12'"], ['complete', 'unclosed', 'opening-only']):
        text = 'A\\' + outer + delim
        if end != 'opening-only':
            text += payload
        if end == 'complete':
            text += delim + 'Z'
        yield dict(family='argument-boundary', outer=outer, delimiter=delim, payload=payload, end=end, source=HEAD + '.No "' + text + '"\n.No AFTER\n' + TAIL)

def produce():
    for kind, operand, label, tail in itertools.product(['tag', 'hang', 'column'], ['', '+0n', '2n', '-2n'], [None, '\\&', '\\:', '\\z', '\\zX', ' ', 'A\\c', 'A'], ['', '.nf\n.fi\n']):
        start = f'.Bl -{kind} -width 8n\n.It Xo\n' if kind != 'column' else '.Bl -column "xxxxxxxx" "xxxx"\n.It Xo\n'
        end = '.Xc\n.No BodyWord\n.El\n' if kind != 'column' else '.Xc Ta RIGHT\n.El\n'
        word = '' if label is None else '.No "' + label + '"\n'
        yield dict(family='ti-transition', kind=kind, operand=operand, label=label, tail=tail, source=HEAD + start + word + '.ti ' + operand + '\n' + tail + '.No AFTER\n' + end + TAIL)
    for kind, label, control in itertools.product(['inset', 'diag', 'ohang'], ['\\&', 'A', 'A\\c'], ['', '.nf\n.fi\n', '.nf\n.No MID\n.fi\n', '.ti\n', '.sp 1\n']):
        s = HEAD + f'.Bl -{kind}\n.It "{label}"\n' + control + '.No AFTER\n.El\n' + TAIL
        yield dict(family='legal-list-controls', kind=kind, label=label, control=control, source=s)
    for depth in [1, 2, 8, 64, 126, 127, 128, 129, 130, 254, 255, 256, 257, 300]:
        s = HEAD + '.No "A' + "\\X'" * depth + 'Q' + "'" * depth + 'Z"\n.No AFTER\n' + TAIL
        yield dict(family='depth', depth=depth, source=s)

def consumers():
    for context, carrier, word in itertools.product(['paragraph', 'tag', 'hang', 'column', 'literal'], ['No', 'Em', 'Sy', 'Lk'], ['\\&\\p AFTER', '\\&\\p \\&\\p AFTER', 'A\\p \\&\\p AFTER', 'AFTER\\p', 'A\\p B']):
        body = f'.{carrier}' + (' https://ex.org' if carrier == 'Lk' else '') + ' "' + word + '"\n'
        if context in ['tag', 'hang']:
            body = f'.Bl -{context} -width 8n\n.It Xo\n' + body + '.Xc\n.No BodyWord\n.El\n'
        if context == 'column':
            body = '.Bl -column "xxxxxxxx" "xxxx"\n.It Xo\n' + body + '.Xc Ta RIGHT\n.El\n'
        if context == 'literal':
            body = '.Bd -literal -compact\n' + body + '.Ed\n'
        yield dict(family='consumer', context=context, carrier=carrier, word=word, source=HEAD + body + TAIL)

def cases():
    """Preserve all 3486 identities and their exact source constructions."""
    for index, case in enumerate(itertools.chain(grammar(), fields(), boundaries(), produce(), consumers())):
        yield dict(case, id=f"review-rule-{index:05d}", rule_id="retained-review",
                   metadata={key: value for key, value in case.items() if key != "source"})

iter_cases = cases
