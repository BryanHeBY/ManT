"""Finite alignment, field, source-node and RS scope products."""
from itertools import product

from .core import COUNTS, END, MAN, MDOC, REQUESTS, request

LIMITS = ('275u', '276u', '277u')


def mode(filled):
    return ('', '') if filled else ('.nf\n', '.fi\n')


def threshold_sources():
    for label, token, shift in product(('A', 'ABCDEFG', 'ABCDEFGHIJKLMNOPQ'), REQUESTS, (-1, 0, 1)):
        limit = 24 * len(label) + 36 + shift  # Fixed ASCII single-column witnesses.
        body = f'.HP {limit}u\n{label}\n' + request(token, '0') + '\\z\n.br\nAFTERMARK\n'
        yield 'E-width', [label, token, shift], MAN + body + END


def mode_sources():
    slots = (
        ('', '.nf\n', '', ''), ('.nf\n', '.fi\n', '', ''),
        ('', '', '.nf\n', ''), ('.nf\n', '', '.fi\n', ''),
        ('', '', '', '.nf\n'), ('.nf\n', '', '', '.fi\n'),
    )
    for slot, token, count, width in product(range(6), REQUESTS, COUNTS, LIMITS):
        start, before, inside, after = slots[slot]
        body = (f'{start}.HP {width}\nBEFOREMARK\n{before}' + request(token, count)
                + f'{inside}\\z\n{after}.br\nAFTERMARK\n.fi\n')
        yield 'E-mode', [slot, token, count, width], MAN + body + END


def space_sources():
    gaps = ('', '.sp -1\n', '.sp -1\n.sp 1\n', '.sp -2\n.sp 1\n.sp 1\n',
            '.sp -1\n.PP\n', '.sp 1\n.PP\n', '.PP\n.PP\n')
    for pd, gap, token, tape in product(('', '0', '1', '2', '-1'), gaps, REQUESTS,
                                        ('', '\\z\n', '.ft B\n.ft R\n')):
        prefix = f'.PD {pd}\n' if pd else ''
        body = ('BEFOREMARK\n' + request(token, '1') + tape + request(token, '0')
                + gap + 'AFTERMARK\n.PD\n')
        yield 'E-space', [pd, gap, token, tape], MAN + prefix + body + END


def node_sources():
    tapes = ('.\\" no node\n', '\n\n', '\nROWONE\n', 'ROWONE\n\n', '\\z\n\\z\n',
             '\\&\\c\n\\&\n', '.B ""\n', '.de AA\n\\&\n\\&\n..\n.AA\n')
    for tape, token, count, filled, context in product(tapes, REQUESTS, COUNTS, (True, False),
                                                       ('', '.HP 4\n', '.HP 16\n')):
        start, end = mode(filled)
        body = 'BEFOREMARK\n' + request(token, count) + tape + '.PP\nAFTERMARK\n'
        yield 'E-node', [tape, token, count, filled, context], MAN + start + context + body + end + END


def owner_sources():
    frames = (
        ('.HP 16\n', '.PP\n'), ('.RS 3\n.HP 16\n', '.RE\n'),
        ('.RS 3\n.RS 2\n.HP 16\n', '.RE\n.RE\n'), ('.IP LABEL 6\n', '.PP\n'),
        ('.TP 6\nLABEL\n', '.PP\n'), ('.Bl -tag -width 6n\n.It LABEL\n', '.El\n'),
        ('.Bl -hang -width 6n\n.It LABEL\n', '.El\n'), ('.Bl -diag\n.It LABEL\n', '.El\n'),
        ('.Bl -column 12n 12n\n.It FIRST Ta SECOND\n', '.El\n'),
        ('.Bd -ragged -offset 4n\n', '.Ed\n'),
    )
    for owner, token, count, tape, inside in product(range(10), REQUESTS, ('0', '1', '2'),
                                                    ('\\z\n', 'ROWONE\n'), (True, False)):
        begin, end = frames[owner]
        heading, footer = (MAN, END) if owner < 5 else (MDOC.replace('.Nd word audit', '.Nd audit'),
                                                       '.Sh END\nENDMARK\n')
        action = '.br\nAFTERMARK\n'
        body = begin + 'BEFOREMARK\n' + request(token, count) + tape
        body += action + end if inside else end + action
        yield 'E-owner', [owner, token, count, tape, inside], heading + body + footer


def tail_sources():
    tails = ('', '.SH NEXT\nAFTERMARK\n', '.PP\nAFTERMARK\n', '.sp -1\n.PP\nAFTERMARK\n',
             '.nf\nAFTERMARK\n.fi\n', '.fi\nAFTERMARK\n', '.br\n.br\nAFTERMARK\n')
    for tail, token, count, width, filled, tape in product(tails, REQUESTS, ('0', '1', '2'),
                                                            ('4', '16'), (True, False),
                                                            ('\\z\n', '\\&\n')):
        begin, end = mode(filled)
        body = f'{begin}.HP {width}\nBEFOREMARK\n' + request(token, count) + tape + tail
        if tail:
            body += end + END
        yield 'E-tail', [tail, token, count, width, filled, tape], MAN + body


def dispatch_sources():
    tapes = ('.br\n', '.nf\n.fi\n', '.ft B\n.ft R\n', '.ll 40n\n', '.mc |\n', '.po +1n\n',
             '.sp 0\n', '.sp -1\n', '.ta 4n 8n\n', '.ti +3n\n')
    for tape, token, count, filled in product(tapes, REQUESTS, ('0', '1', '2'), (True, False)):
        begin, end = mode(filled)
        body = (begin + 'BEFOREMARK\n' + request(token, count) + tape
                + '.PP\n.mc\n.ll 78n\n.po 0\nAFTERMARK\n' + end)
        yield 'E-dispatch', [tape, token, count, filled], MAN + body + END


def rs_sources():
    scopes = (('.RS 0\n', '.RE\n'), ('.RS 3\n', '.RE\n'), ('.RS 3\n.RS 2\n', '.RE\n.RE\n'))
    for scope, inside, before, after, exit_break in product(range(3), ('', '.PP\n'), ('', '.PP\n'),
                                                            ('', '.PP\n'), ('', '.br\n', '.br\n.br\n')):
        begin, end = scopes[scope]
        body = ('OUTERMARK\n' + before + begin + 'BEFOREMARK\n' + inside + 'INSIDEMARK\n'
                + end + exit_break + after + 'AFTERMARK\n')
        yield 'RS-review', [scope, inside, before, after, exit_break], MAN + body + END
