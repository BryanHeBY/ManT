"""Source-bound word routes, ordered cuts, and field bridges from the review."""
from itertools import product
import re

from .core import MAN, MDOC, PREFIX, REQUESTS, SPACING, TAIL, request

WORD_MAN = MAN.replace(r'\- audit', r'\- word audit')
SEEDS = (
    (PREFIX[0], ''), (PREFIX[1], r'\:'), (PREFIX[2], r'\:'),
    (PREFIX[2], '\\&\\p\\: '), (PREFIX[2], '\\z \\p '),
    (PREFIX[2], '\\&\\p\\: \\p '), (PREFIX[2], '\\&\\p\\: \\zY\\p '),
    (PREFIX[3], '\\&\\p\\: \\zY\\p '), (PREFIX[4], '\\&\\p\\: \\p '),
    (r'ALPHA\p\:\zX', '\\&\\p\\: \\p '),
    (PREFIX[2], '\\&\\p\\: \\zY\\p\\: \\p '),
    (PREFIX[2], '\\&\\p\\: \\p\\: \\p\\: \\p '),
)
RETIRE = ('.br\n.No OMEGA\n', *TAIL)
CONTROL = re.compile(r'\\[zp:&]|.', re.DOTALL)
LINK = 'https://matrix.test/w'


def words(before, after, macro='No'):
    return f'.No "{before}"\n.{macro} "{after}"\n'


def copy_escape(text):
    return text.replace('\\', '\\\\')


def template_routes(before, after):
    pair = words(before, after)
    yield f'.No "{before}" No "{after}"\n'
    yield f'.No "{before}"\n{after}\n'
    yield f'.No "{before}" Ns No "{after}"\n'
    for filler in ('.No ""\n', '.No ""\n.No ""\n', '.No "\\fB"\n'):
        yield f'.No "{before}"\n{filler}.No "{after}"\n'
    for wrapper in ('Em', 'Li', f'Lk {LINK}'):
        yield words(before, after, wrapper)
    for start, end in (('.Bf -emphasis\n', '.Ef\n'), ('.Bd -filled\n', '.Ed\n'),
                       ('.Bd -literal\n', '.Ed\n')):
        yield start + pair + end
    for style in ('tag', 'hang'):
        for head, end in (('.It Xo\n', '.Xc\nBODY\n.El\n'), ('.It HEAD\n', '.El\n')):
            yield f'.Bl -{style} -width 20n\n{head}{pair}{end}'
    for style in ('inset', 'diag'):
        yield f'.Bl -{style}\n.It HEAD\n{pair}.El\n'
    yield '.Bl -column "20n" "10n"\n.It HEAD Ta\n' + pair + '.El\n'
    yield '.de SEAM\n' + copy_escape(pair) + '..\n.SEAM\n'
    yield f'.Nm "{before}" "{after}"\n'
    yield f'.Fn f "{before}" "{after}"\n'
    yield f'.Fo f\n.Fa "{before}"\n.Fa "{after}"\n.Fc\n'
    yield f'.Sh "{before}" "{after}"\n'
    for start, end in (('', ''), ('.nf\n', '.fi\n')):
        for token, target, close in (('UR', LINK, 'UE'), ('MT', 'u@matrix.test', 'ME')):
            yield f'{start}.{token} {target}\n.B "{before}"\n.B "{after}"\n.{close}\n{end}'
    yield f'.TS\nl.\nT{{\n{before}\n{after}\nT}}\n.TE\n'
    yield f'.No "{before}"\n.EQ\nx\n.EN\n.No "{after}"\n'


def route_sources():
    frame = (('', ''), ('.Bd -literal\n', '.Ed\n'), ('.Bd -filled\n', '.Ed\n'),
             ('.Bl -tag -width 20n\n.It HEAD\n', '.El\n'),
             ('.Bl -hang -width 20n\n.It HEAD\n', '.El\n'),
             ('.Bl -inset\n.It HEAD\n', '.El\n'))
    for seed, (before, control) in enumerate(SEEDS):
        after = control + 'BETA'
        for route, body in enumerate(template_routes(before, after)):
            man = 24 <= route <= 27
            for ending, tail in enumerate(RETIRE):
                if man:
                    # Raw OMEGA and .B "" visit the same dialect.
                    tail = tail.replace('.No OMEGA', 'OMEGA').replace('.No ""', '.B ""')
                yield 'A-route', [seed, route, ending], (WORD_MAN if man else MDOC) + body + tail
        for outer, depth, ending in product(range(6), (1, 2, 4), range(3)):
            start, end = frame[outer]
            body = ('.Bf -emphasis\n' * depth + words(before, after, f'Lk {LINK}')
                    + '.Ef\n' * depth)
            yield 'A-nested', [seed, outer, depth, ending], MDOC + start + body + end + RETIRE[ending]


def spacing_sources():
    scripts = (
        ('', '', ''), ('', '.Sm off\n', '.Sm on\n'),
        ('.Sm off\n', '.Sm on\n', ''), ('', '.Bk -words\n', '.Ek\n'),
        ('.Bk -words\n', '.Ek\n', ''), ('', 'END.\n', ''),
    )
    retire = ('.No OMEGA\n', '.sp 0\n', '.sp 1\n', '.Pp\n', '.nf\n.fi\n',
              '.Sh NEXT\n', '.br\n.br\n', '.sp 0\n.sp 0\n')
    for seed, (before, control) in enumerate(SEEDS):
        after = control + 'BETA'
        for s, ending in product(range(6), range(3)):
            start, mid, end = scripts[s]
            body = f'{start}.No "{before}"\n{mid}.No "{after}"\n'
            yield 'A-spacing', [seed, s, ending], MDOC + body + RETIRE[ending] + end
        for punctuation, ending in product(('END.\n', '.No "END."\n'), range(3)):
            body = f'.No "{before}"\n{punctuation}.No "{after}"\n'
            yield 'A-sentence', [seed, punctuation, ending], MDOC + body + RETIRE[ending]
        for boundary, spacing in product(range(8), range(3)):
            start, end = SPACING[spacing]
            yield 'A-retire', [seed, boundary, spacing], MDOC + start + words(
                before, after) + retire[boundary] + end


def cut_sources():
    seams = ('"\n.No "', '" No "', '"\n.No ""\n.No "', '" Ns No "')
    for seed, (before, control) in enumerate(SEEDS):
        tokens = CONTROL.findall(control)
        if not tokens:
            continue
        cut_sets = [(i, {i}) for i in range(1, len(tokens))]
        cut_sets.append(('all', set(range(1, len(tokens)))))
        for (name, cuts), seam, ending in product(cut_sets, range(4), range(3)):
            after = ''.join((seams[seam] if i in cuts else '') + token
                            for i, token in enumerate(tokens)) + 'BETA'
            yield 'A-token-cut', [seed, name, seam, ending], MDOC + words(before, after) + RETIRE[ending]


def glyph_sources():
    glyphs = (
        ('X', 'Y'), ('界', '字'), ('e\u0301', 'o\u0308'),
        (r'\[u00E9]', r'\[u00F6]'), (r"\o'AB'", r"\o'CD'"), (r'\fBX\fP', r'\fIY\fP'),
    )
    controls = (r'\c', r"\h'0'", r"\h'1n'", r"\h'-1n'", r'\fB\fP', r'\p\&',
                r'\p\&\:', r'\&\:')
    for seed, (before, control) in enumerate(SEEDS):
        spaces = ['all', *(i for i, character in enumerate(control) if character == ' ')]
        for position, blank, tail in product(spaces, (' ', '\t', r'\~', r'\ '), range(2)):
            converted = ''.join(blank if character == ' ' and position in ('all', i)
                                else character for i, character in enumerate(control))
            yield 'A-blank', [seed, position, blank, tail], MDOC + words(
                before, converted + 'BETA') + TAIL[tail]
        for index, tail in product(range(6), range(2)):
            x, y = glyphs[index]
            replace = str.maketrans({'X': x, 'Y': y})
            yield 'A-glyph', [seed, index, tail], MDOC + words(
                before.translate(replace), control.translate(replace) + 'BETA') + TAIL[tail]
        for added, tail in product(controls, range(2)):
            yield 'A-control', [seed, added, tail], MDOC + words(before, added + control + 'BETA') + TAIL[tail]
        for width, pad in product((20, 40, 78, 160), (0, 1, 7, 15)):
            yield 'A-width', [seed, width, pad], MDOC + words('Q' * pad + before, control + 'BETA')


def delivery_sources():
    for seed, (before, control) in enumerate(SEEDS):
        after = control + 'BETA'
        direct = words(before, after)
        escaped = copy_escape(direct)
        alternatives = (
            direct,
            f'.ds PX {copy_escape(before)}\n.ds CX {copy_escape(after)}\n.No "\\*[PX]"\n.No "\\*[CX]"\n',
            '.de ARGS\n.No "\\\\$1"\n.No "\\\\$2"\n..\n' + f'.ARGS "{before}" "{after}"\n',
            f'.de INNER\n{escaped}..\n.de OUTER\n.INNER\n..\n.OUTER\n',
        )
        for carrier, ending in product(range(4), range(3)):
            yield 'A-delivery', [seed, carrier, ending], MDOC + alternatives[carrier] + RETIRE[ending]


def scale_sources():
    for count, tail in product((32, 64, 128), range(2)):
        tapes = (
            words(PREFIX[2], '\\p ' * count + 'BETA'),
            words(PREFIX[2], '\\&\\p\\: ' * count + 'BETA'),
            words(PREFIX[2], '\\&\\p\\: \\zY\\p ' * count + 'BETA'),
            f'.No "{PREFIX[2]}"\n' + '.No ""\n' * count + '.No BETA\n',
            f'.No "{PREFIX[2]}"\n' + '.No "\\&\\p\\:"\n' * count + '.No BETA\n',
            words(PREFIX[2], r'\&' * count + 'BETA'),
        )
        for family, body in enumerate(tapes):
            yield 'A-scale', [family, count, tail], MDOC + body + TAIL[tail]


def bridge_sources():
    fields = ('.PP\n', '.HP 275u\n', '.HP 276u\n', '.HP 277u\n')
    tails = ('.br\nAFTERMARK\n', '.PP\nAFTERMARK\n', '.SH AFTER\nAFTERMARK\n', '')
    for seed, field, token, count, carrier, tail in product(
            (2, 3, 4, 5, 6, 10), fields, REQUESTS, ('0', '1', '2'), ('\\z\n', '\\&\n'), tails):
        before, control = SEEDS[seed]
        after = control + 'BETA'
        operands = f'.B "{before}"\n.B "{after}"\n'
        alignment = request(token, count)
        bodies = (operands + alignment + carrier,
                  'BEFOREMARK\n' + alignment + carrier + operands,
                  'BEFOREMARK\n' + alignment + f'{before}\n{after}\n' + carrier)
        for placement, body in enumerate(bodies):
            yield 'bridge', [seed, field, token, count, carrier, tail, placement], WORD_MAN + field + body + tail
