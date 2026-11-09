"""The six finite core products, with coordinates retained across aliases."""
from itertools import product

MDOC = ('.Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n'
        '.Nd word audit\n.Sh DESCRIPTION\n')
MAN = '.TH CHECK 1 "2026-11-10"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n'
END = '.SH END\nENDMARK\n'
PREFIX = ('ALPHA', r'ALPHA\z', r'ALPHA\zX', r'ALPHA\zX\z',
          'ALPHA\\zX\\p\\: ', 'ALPHA\\zX\\p\\: \\zY')
PHASE = (r'\p', '\\p ', '\\p  ', ' \\p', r'\p\:', '\\&\\p\\: ')
V = (PHASE[1], PHASE[4], PHASE[5])
SPACING = (('', ''), ('.Sm off\n', '.Sm on\n'), ('.Bk -words\n', '.Ek\n'))
TAIL = ('.No ""\n.br\n.No OMEGA\n', '')

COUNTS = ("", "0", "1", "2", "-1", "x")
REQUESTS = ("ce", "rj")


def request(token, count):
    return "." + token + (" " + count if count else "") + "\n"


def common(block, seq_id, payload):
    for p, seam, spacing, tail in product(range(6), range(2), range(3), range(2)):
        body = (f'.No "{PREFIX[p]}{payload}BETA"\n' if seam == 0
                else f'.No "{PREFIX[p]}"\n.No "{payload}BETA"\n')
        source = MDOC + SPACING[spacing][0] + body + TAIL[tail] + SPACING[spacing][1]
        yield block, [seq_id, p, seam, spacing, tail], source

def core_sources():
    for k in range(5):
        for labels in product(range(5), repeat=k):
            tape = ''.join((r'\z', r'\p', r'\:', r'\&', ' ')[i] for i in labels)
            yield from common('A-atomic', list(labels), tape)
    for k in (2, 3, 4):
        for labels in product(range(6), repeat=k):
            yield from common('A-phase', list(labels), ''.join(PHASE[i] for i in labels))
        for labels in product(range(3), repeat=k):
            phases = [V[i] for i in labels]
            for cut_name, cuts in [(str(i), {i}) for i in range(1, k)] + [('all', set(range(1, k)))]:
                for insertion in (r'\z', r'\zY', r'\zY\z'):
                    tape = phases[0] + ''.join((insertion if i in cuts else '') + phases[i] for i in range(1, k))
                    yield from common('A-rearm', [list(labels), cut_name, insertion], tape)
                groups = [phases[0]]
                for i in range(1, k):
                    if i in cuts:
                        groups.append(phases[i])
                    else:
                        groups[-1] += phases[i]
                groups[-1] += 'BETA'
                for p, spacing, tail, seam in product(range(6), range(3), range(2), range(4)):
                    join = ('"\n.No "', '" No "', '"\n.No ""\n.No "', '" Ns No "')[seam]
                    body = f'.No "{PREFIX[p]}"\n.No "' + join.join(groups) + '"\n'
                    source = MDOC + SPACING[spacing][0] + body + TAIL[tail] + SPACING[spacing][1]
                    yield 'A-seam', [list(labels), cut_name, p, spacing, tail, seam], source
    carriers = ('', '\\z\n', '\\zX\n', '\\&\n', '\\p\n', 'ROWONE\n')
    tails = ('', '.br\n', '.br\n.br\n', '.PP\n', '.sp 0\n', '.sp 1\n')
    for h,q,k,filled,c,t in product(('4n','275u','276u','277u','24n'), REQUESTS, COUNTS, (True,False), range(6), range(6)):
        enter,leave = ('','') if filled else ('.nf\n','.fi\n')
        source = MAN + enter + '.HP '+h+'\nBEFOREMARK\n' + request(q,k) + carriers[c] + tails[t] + 'AFTERMARK\n' + leave + END
        yield 'B-core', [h,q,k,filled,c,t], source
    tapes = ('','\n','   \n','\\z\n','\\&\n','ROWONE\n','\\z\nROWONE\n','.ft B\n.ft R\n')
    gaps = ('','.PD 0\n.PP\n.PD\n','.PP\n','.sp 0\n','.sp 1\n','.sp 2\n')
    for q,k,filled,t,pos,g in product(REQUESTS, COUNTS, (True,False), range(8), ('start','middle'), range(6)):
        enter,leave = ('','') if filled else ('.nf\n','.fi\n')
        source = MAN + enter + ('' if pos=='start' else 'BEFOREMARK\n') + request(q,k) + tapes[t] + request(q,'0') + gaps[g] + 'AFTERMARK\n' + leave + END
        yield 'C-core', [q,k,filled,t,pos,g], source
