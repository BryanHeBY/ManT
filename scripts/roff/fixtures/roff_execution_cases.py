#!/usr/bin/env python3
"""Canonical source definitions for formatter and link regression matrices.

These are historical complete inputs. Headers, quoting, controls and malformed
recovery cases are deliberately preserved; lint metadata decides assertion scope.
Changing a definition requires pristine-oracle regeneration, never product gold.
"""
import itertools

HEAD = {
    'mdoc': '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n',
    'man': '.TH TEST 1 "September 28, 2026"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n',
}

def q(s):
    return '"' + s + '"'

def operand(dialect, carrier, value):
    if carrier == 'raw':
        return value + '\n'
    if carrier == 'Lk':
        return '.Lk https://ex.org ' + q(value) + '\n'
    if carrier == 'UR':
        return '.UR https://ex.org\n' + value + '\n.UE\n'
    return '.' + carrier + ' ' + q(value) + '\n'

def surround(dialect, context, content):
    if context == 'filled':
        return content
    if context == 'nf':
        return '.nf\n' + content + '.fi\n'
    if context == 'literal':
        return '.Bd -literal\n' + content + '.Ed\n'
    if context == 'unfilled':
        return '.Bd -unfilled\n' + content + '.Ed\n'
    if context == 'EX':
        return '.EX\n' + content + '.EE\n'
    if context in ('tag', 'hang'):
        return '.Bl -' + context + ' -width 8n\n.It Xo\n' + content + '.Xc\n.No BodyWord\n.El\n'
    if context in ('tag-body', 'hang-body'):
        return '.Bl -' + context.split('-')[0] + ' -width 8n\n.It HeadWord\n' + content + '.El\n'
    if context == 'column':
        return '.Bl -column "xxxx" "xxxx"\n.It Xo\n' + content + '.Xc\n.Ta RightWord\n.El\n'
    if context == 'TP':
        return '.TP 8\nHeadWord\n' + content
    if context == 'RS':
        return '.RS 4\n' + content + '.RE\n'
    if context == 'tbl':
        return '.TS\nl.\nT{\n' + content + 'T}\n.TE\n'
    raise ValueError(context)

def case(family, dialect, context, body, **axes):
    return dict(family=family, dialect=dialect, context=context, **axes,
                source=HEAD[dialect]+surround(dialect,context,body)+
                ('.Sh NEXT\n.No END\n' if dialect=='mdoc' else '.SH NEXT\nEND\n'))

def word_cases():
    prefixes = ['', 'BEFORE', r'BEFORE\c', r'BEFORE \c', r'BEFORE\p\c',
                r'\&\c', r'\~\c', r'\0\c', r'\zP\c', r'\z\c', r'\p\c', r'中\c']
    words = [r'\p D', r'\p  D', r'\pD', r'\p\& D', r'\p\~D', r'D\p E',
             r'\p D\p E', r'\p\p D', r'\zP\p  D', 'D']
    for dialect in HEAD:
        contexts = ['filled','nf','literal','unfilled'] if dialect=='mdoc' else ['filled','nf','EX']
        for context,pre,word in itertools.product(contexts,prefixes,words):
            yield case('word-boundaries',dialect,context,(pre+'\n' if pre else '')+word+'\nAFTER\n',prefix=pre,word=word)
    for dialect in HEAD:
        contexts = ['nf','literal','tag','hang','column'] if dialect=='mdoc' else ['nf','EX','TP','RS','tbl']
        carriers = ['No','Em','Lk'] if dialect=='mdoc' else ['B','I','UR']
        for context,carrier,pre,word in itertools.product(contexts,carriers,
                [r'BEFORE\c',r'BEFORE \c',r'\&\c',r'\zP\c'],[r'\p D',r'\p  D',r'D\p E',r'\p\& D']):
            yield case('macro-operands',dialect,context,pre+'\n'+operand(dialect,carrier,word)+'AFTER\n',carrier=carrier,prefix=pre,word=word)
    for dialect in HEAD:
        for pre,word,boundary in itertools.product(
                [r'BEFORE\c',r'\&\c',r'\zP\c',r'BEFORE \c'],
                [r'\p D',r'\p  D',r'D\p E'],
                ['', '.br\n','.sp 0\n','.sp 1\n','.nf\n','.fi\n','.ft B\n','.mc |\n','.ti 2n\n']):
            yield case('control-lifecycle',dialect,'nf',pre+'\n'+boundary+word+'\nAFTER\n',prefix=pre,word=word,boundary=boundary)
    labels=['',r'\&',r'\zX',r'\p D',r'D\p E',r'D \p E',r'D\p \p',r'\p',r'D\zX',r'\[u0301]','label']
    for context,label,boundary in itertools.product(
            ['filled','nf','literal','tag','hang','column','tag-body','hang-body'],labels,
            ['','.br\n','.Pp\n']):
        yield case('link-identities','mdoc',context,'.Lk https://ex.org '+q(label)+'\n'+boundary+'.No AFTER\n',label=label,boundary=boundary)
    for context,prefix,label in itertools.product(
            ['filled','nf','literal','tag','hang','column'],
            ['.Sm off\n','.No BEFORE Ns\n','.No \\zP\n','.No BEFORE\\c\n'],
            ['',r'\p D',r'\p  D',r'D\p E',r'\zX',r'D\zX']):
        yield case('link-joins','mdoc',context,prefix+'.Lk https://ex.org '+q(label)+'\n.No AFTER\n',prefix=prefix,label=label)
    for context,boundary,label in itertools.product(
            ['filled','nf','literal','tag','hang','column'],['','.br\n','.Pp\n'],
            [r'\p D',r'D\p E','']):
        line='.Lk https://ex.org '+q(label)+'\n'
        yield case('independent-links','mdoc',context,line+boundary+line+'.No AFTER\n',label=label,boundary=boundary)
    # Changes of Rust output destination are not implicitly native flushes.
    for pre,word,container in itertools.product(
            [r'BEFORE\c',r'BEFORE \c',r'\&\c'],[r'\p D',r'\p  D',r'D\p E'],
            ['font','nested-literal','tag','hang','column','RS']):
        middle=('.Bf -emphasis\n'+word+'\n.Ef\n' if container=='font' else
                '.Bd -literal\n'+word+'\n.Ed\n' if container=='nested-literal' else
                surround('mdoc',container,word+'\n'))
        yield case('output-owners','mdoc','nf',pre+'\n'+middle+'AFTER\n',prefix=pre,word=word,inner=container)

def column_cases():
    for prefix, label, carrier, boundary in itertools.product(
        ['', '.No \\zP\n', '.No BEFORE\\c\n', '.No BEFORE \\c\n'],
        [r'\p D',r'\p  D',r'D\p \p',r'D\p E',r'\p\& D','label'],
        ['No','Lk'],['','.br\n','.Pp\n']):
        yield case('shared-words','mdoc','filled',prefix+operand('mdoc',carrier,label)+boundary+'.No AFTER\n',prefix=prefix,label=label,carrier=carrier,boundary=boundary)
    for carrier,label,boundary,width,rows in itertools.product(
        ['No','Lk'],[r'\p D',r'\p  D',r'D\p E',r'D\p \p','label'],
        ['','.br\n','.Pp\n'],[4,8,20],['inline','multiline']):
        item=(f'.It {carrier} '+('https://ex.org ' if carrier=='Lk' else '')+q(label)+' No AFTER Ta RightWord\n')
        if rows=='multiline' or boundary:
            item='.It Xo\n'+operand('mdoc',carrier,label)+boundary+'.No AFTER\n.Xc Ta RightWord\n'
        body=f'.Bl -column "{ "x"*width }" "xxxx"\n'+item+'.El\n'
        yield case('column-bodies','mdoc','filled',body,carrier=carrier,label=label,boundary=boundary,width=width,rows=rows)
    for kind, label,carrier,boundary,width in itertools.product(
        ['tag','hang'],['LABEL','VERYVERYLONGLABEL',r'D\p \p',r'D \p E'],
        ['No','Lk'],['.br\n','.Pp\n','.sp 1\n'],[4,8,32]):
        body=f'.Bl -{kind} -width {width}n\n.It Xo\n'+operand('mdoc',carrier,label)+boundary+'.No AFTER\n.Xc\n.No BodyWord\n.El\n'
        yield case('head-requests','mdoc','filled',body,kind=kind,label=label,carrier=carrier,boundary=boundary,width=width)

def link_cases():
    header = '.Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n'
    labels = ['', r'\&', r'\z', r'\zX', r'\zXY', r'X\z', r'X\zY',
              r'\pX', r'X\p', r'X\pY', r'X\p Y', r'X\c', 'one two',
              r'X\~Y', r'X\0Y', '   ', r'X\p\&']
    urls = ['https://e.test', r'https://e.test/\c', r'\zXhttps://e.test',
            r'https://e.test/\p', '']
    contexts = {
        'plain': lambda text: 'BEFORE\n' + text + '\n.No AFTER\n',
        'nf': lambda text: '.nf\nBEFORE\n' + text + '\n.No AFTER\n.fi\n',
        'tag': lambda text: '.Bl -tag -width 8n\n.It ' + text[1:] + '\nBodyWord\n.El\n',
        'hang': lambda text: '.Bl -hang -width 8n\n.It ' + text[1:] + '\nBodyWord\n.El\n',
    }
    for context, (label, url) in itertools.product(contexts, itertools.product(labels, urls)):
        text = '.Lk "' + url + '" "' + label + '"'
        yield dict(family='link-operands', dialect='mdoc', context=context,
                   label=label, uri=url,
                   source=header+contexts[context](text)+'.Sh ENDTEST\nDONE\n')
    prefixes = [r'X', r'\p X', r'X\p Y', r'\p', r'\zX', r'X \z',
                r'\p X\c', r'X\c', r'\&', r' ', r'X\p\p Y']
    labels = ['label', r'\p label', r'X\p Y', r'\p', r'\zX', '', r'\&', r'X\c']
    for kind, width, prefix, label in itertools.product(['tag', 'hang'], [1, 4, 8], prefixes, labels):
        body = (f'.Bl -{kind} -width {width}n\n.It No "{prefix}" '
                f'Lk https://e.test "{label}" No ENDHEAD\nBODY\n.El\n')
        yield dict(family='link-head-fields', dialect='mdoc', context=kind,
                   width=width, prefix=prefix, label=label,
                   source=header+body+'.Sh ENDTEST\nEND\n')


def matrices():
    yield 'formatter', itertools.chain(word_cases(), column_cases())
    yield 'links', link_cases()
