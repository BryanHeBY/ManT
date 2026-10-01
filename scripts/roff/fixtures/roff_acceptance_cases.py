#!/usr/bin/env python3
"""Frozen acceptance-matrix source definitions for the shared-execution repair.

This module extends the canonical generator set (``roff_execution_cases``)
with two cohorts:

* ``adjacent`` families cell-ownership / head-control-requests /
  column-tail-rows (guide section 5.1 sets M/N/O).  These reproduce, byte for
  byte, the adjacent review evidence recorded at
  ``review-evidence/roff-shared-execution-98e550ae-20261001/adjacent.jsonl``;
  the generation script and JSONL are the specification, not the summary
  tables.
* ``expansion`` families wrapper-entries / cross-owner-words /
  column-positions / spacing-intervals / delimiter-arguments /
  repeated-link-identities / physical-row-handoffs (guide section 5.6 sets
  XW/XO/XC/XV/XE/XI/XR) plus the three named skipvsp RESET contrasts derived
  from XV.  Axis values and construction rules follow section 5.6 verbatim.

Sources are the specification.  Axis values, templates and assertion policies
are frozen here and never re-derived from product behaviour.  Assertion axes
use the section 5.7 vocabulary; the replay recorder turns them into the
admission ledger.  No gold is written from these definitions alone: the
registered pristine oracle must run first (section 5.6 admission order).
"""

import itertools

from scripts.roff.fixtures.roff_execution_cases import HEAD, operand, surround, case


# --------------------------------------------------------------------------
# Adjacent cohort (guide 5.1: sets M, N, O; evidence adjacent.py is the spec)
# --------------------------------------------------------------------------

M_PREFIXES = ['', r'\zP', r'\zP\c', r'\z中', r'\z\[u0301]', r'\zP\z', r'X\p', r'X \p']
M_WORDS = [r'\p  Y', r'\p\& Y', r'\p\~Y', r'\p\0Y', r'\p\[u0301] Y',
           r'\p\[u4E2D] Y', r'\p\[u200B] Y', r'\p\: Y', r'\p\fB Y',
           r'\p\zY Z', r'\p\o\(aqXY\(aq Z', r'Y\p \&', r'Y\p\& \p Z', 'Y\\p\tZ']
M_CONTEXTS = ['filled', 'nf', 'tag', 'hang']
M_CARRIERS = ['No', 'Em', 'Lk']

N_KINDS = ['tag', 'hang']
N_WIDTHS = [4, 8, 32]
N_LABELS = [r'D \p E', r'D\p \p', r'D\p E', 'LABEL', '']
N_CONTROLS = ['.br\n', '.sp 0\n', '.sp 1\n', '.sp 2\n', '.Pp\n', '.br\n.br\n', '.sp 1\n.sp 1\n']

O_WIDTHS = [4, 8, 20]
O_LABELS = [r'D\p \p', r'D \p E', r'D\p E', r'\p E', r'\&', 'D']
O_SUFFIXES = ['', ' No AFTER', r' No \&', ' No ""']
O_LAYOUTS = ['inline', 'multiline']


def cell_ownership_cases():
    """Set M: actual native word/cell kinds under each owner carrier."""
    for context, prefix, word, carrier in itertools.product(
            M_CONTEXTS, M_PREFIXES, M_WORDS, M_CARRIERS):
        body = ('.No "' + prefix + '"\n' if prefix else '') \
            + operand('mdoc', carrier, word) + '.No AFTER\n'
        yield case('cell-ownership', 'mdoc', context, body,
                   prefix=prefix, word=word, carrier=carrier)


def head_control_request_cases():
    """Set N: tag/hang HEAD payloads followed by row/spacing control requests."""
    for kind, width, label, control, carrier in itertools.product(
            N_KINDS, N_WIDTHS, N_LABELS, N_CONTROLS, ['No', 'Lk']):
        body = (f'.Bl -{kind} -width {width}n\n.It Xo\n'
                + operand('mdoc', carrier, label) + control
                + '.No AFTER\n.Xc\n.No BodyWord\n.El\n')
        yield case('head-control-requests', 'mdoc', 'filled', body,
                   kind=kind, width=width, label=label, control=control,
                   carrier=carrier)


def column_tail_row_cases():
    """Set O: final real hard row of a column field versus following cells."""
    for width, label, suffix, layout in itertools.product(
            O_WIDTHS, O_LABELS, O_SUFFIXES, O_LAYOUTS):
        item = f'.It No "{label}"{suffix} Ta RightWord\n'
        if layout == 'multiline':
            item = ('.It Xo\n.No "' + label + '"\n'
                    + ('.' + suffix.strip() + '\n' if suffix else '')
                    + '.Xc Ta RightWord\n')
        body = f'.Bl -column "{"x" * width}" "xxxx"\n' + item + '.El\n'
        yield case('column-tail-rows', 'mdoc', 'filled', body,
                   width=width, label=label, suffix=suffix, layout=layout)


# --------------------------------------------------------------------------
# Expansion cohort (guide 5.6: XW/XO/XC/XV/XE/XI/XR plus XV-derived contrasts)
# --------------------------------------------------------------------------

XW_WRAPPERS = [
    ('mdoc', 'Lk', '.Lk https://ex.org LABEL'),
    ('mdoc', 'Mt', '.Mt x@example.org'),
    ('mdoc', 'Sx', '.Sx TARGET'),
    ('mdoc', 'In', '.In stdio.h'),
    ('mdoc', 'Bx', '.Bx 4.4'),
    ('mdoc', 'Xr', '.Xr printf 3'),
    ('man', 'MR', '.MR printf 3'),
]
XW_PRE_STATES = ['', r'\zP', r'\zP\z', r'BEFORE\c']
XW_POST_CONTROLS = [('none', ''), ('br', '.br\n'), ('sp1', '.sp 1\n'), ('para', None)]

XO_CARRIERS = ['No', 'Em', 'Lk']
XO_PAYLOADS = ['Y', r'\p  Y', r'Y\p\& \p Z', r'D \p E', r'\&', '']
XO_CONTEXTS = ['filled', 'nf', 'tag', 'hang']

XC_POSITIONS = ['first', 'middle', 'last']
XC_LAYOUTS = ['inline', 'multiline']
XC_AUX_LEFT = 'AUX_LEFT'
XC_AUX_RIGHT = 'AUX_RIGHT'

XV_HEADS = [r'D \p E', r'D\p \p', r'D\p E', 'LABEL', '', r'\&']
XV_CONTROLS = [
    ('br', '.br\n'),
    ('sp0', '.sp 0\n'),
    ('sp1', '.sp 1\n'),
    ('sp2', '.sp 2\n'),
    ('sp1-sp2', '.sp 1\n.sp 2\n'),
    ('pp', '.Pp\n'),
    ('spneg1-sp1', '.sp -1\n.sp 1\n'),
    ('spneg1-reset-sp1', '.sp -1\n.No RESET\n.sp 1\n'),
]
XV_HEAD_CARRIERS = ['No', 'Lk']
XV_BODY_CARRIERS = [('no', '.No BodyWord\n'),
                    ('lk', '.Lk https://body.example BodyWord\n')]
# Named XV-derived contrasts (guide 5.6): the real RESET word inside the
# spneg1-reset-sp1 control is replaced by checkpoint-only / empty / zero-width
# inputs.  These are directed contrasts outside the frozen 1,152 count.
XV_RESET_VARIANTS = [
    ('wrapper-checkpoint', '.Lk https://reset.example ""\n'),
    ('empty-operand', '.No ""\n'),
    ('zero-width-word', '.No "\\zR"\n'),
]

XE_DELIMITERS = [
    ('literal-apostrophe', "'", None),
    ('literal-pipe', '|', None),
    ('literal-bang', '!', None),
    ('escape-aq', r'\(aq', r'\('),
    ('bracket-aq', r'\[aq]', r'\[a'),
]
XE_PAYLOADS = ['', 'XY', r'X\zY']
XE_PREFIXES = ['', r'\p']
XE_TERMINATIONS = ['complete', 'missing-close', 'truncated-close']

XI_CONTROLS = [('none', ''), ('br', '.br\n'), ('pp', '.Pp\n')]
XI_LABELS = ['LABEL', '', r'\&', r'\p D']
XI_TARGET_PAIRS = [
    ('same', 'https://ex.org', 'https://ex.org'),
    ('different', 'https://ex.org', 'https://alt.example'),
]
XI_CONTEXTS = ['filled', 'tag', 'column']

XR_INPUTS = ['empty-source-line', 'empty-operand', 'ampersand', 'bare-z']
XR_EXITS = ['none', 'br', 'sp1', 'para', 'section']
XR_TAILS = ['after', 'eof']


def wrapper_entry_cases():
    """XW: seven legal wrappers x four pre-states x four post controls."""
    for (dialect, wrapper, line), prefix, (post, control) in itertools.product(
            XW_WRAPPERS, XW_PRE_STATES, XW_POST_CONTROLS):
        if prefix:
            pre_line = f'.No "{prefix}"\n' if dialect == 'mdoc' else prefix + '\n'
        else:
            pre_line = ''
        if control is None:
            control = '.Pp\n' if dialect == 'mdoc' else '.PP\n'
        after = '.No AFTER\n' if dialect == 'mdoc' else 'AFTER\n'
        source = HEAD[dialect] + pre_line + line + '\n' + control + after
        if dialect == 'mdoc':
            source += '.Sh NEXT\n.No END\n'
            if wrapper == 'Sx':
                # A real TARGET section exists so Sx address resolution is
                # never confused with a target-less reference.
                source += '.Sh TARGET\n.No TARGETBODY\n'
        else:
            source += '.SH NEXT\nEND\n'
        yield dict(family='wrapper-entries', dialect=dialect, wrapper=wrapper,
                   prefix=prefix, post=post, source=source)


def cross_owner_word_cases():
    """XO: visible operands crossing owner entry with pending/rejected state."""
    for carrier, prefix, payload, context, (post, post_control) in itertools.product(
            XO_CARRIERS, XW_PRE_STATES, XO_PAYLOADS, XO_CONTEXTS, XW_POST_CONTROLS):
        control = '.Pp\n' if post_control is None else post_control
        pre_line = f'.No "{prefix}"\n' if prefix else ''
        body = pre_line + operand('mdoc', carrier, payload) + control + '.No AFTER\n'
        yield case('cross-owner-words', 'mdoc', context, body, carrier=carrier,
                   prefix=prefix, payload=payload, post=post)


def _xc_tested_inline(carrier, payload, suffix):
    if carrier == 'Lk':
        return f'Lk https://ex.org "{payload}"{suffix}'
    return f'No "{payload}"{suffix}'


def _xc_multiline(carrier, payload, suffix):
    line = operand('mdoc', carrier, payload)
    suffix_line = ('.' + suffix.strip() + '\n') if suffix else ''
    return line + suffix_line + '.Xc'


def column_position_cases():
    """XC: three-column rows with the tested cell at first/middle/last."""
    for width, payload, suffix, layout, position, carrier in itertools.product(
            O_WIDTHS, O_LABELS, O_SUFFIXES, XC_LAYOUTS, XC_POSITIONS, ['No', 'Lk']):
        if layout == 'inline':
            tested = _xc_tested_inline(carrier, payload, suffix)
            # The two untested columns carry distinct sentinels in reading
            # order, mirroring the multi-line template below.
            sentinels = iter((XC_AUX_LEFT, XC_AUX_RIGHT))
            cells = {key: (tested if key == position else next(sentinels))
                     for key in ('first', 'middle', 'last')}
            row = ' Ta '.join(cells[key] for key in ('first', 'middle', 'last'))
            body = ('.Bl -column "' + 'x' * width + '" "xxxx" "xxxx"\n.It '
                    + row + '\n.El\n')
        else:
            # The tested cell is a multi-line Xo/Xc block; template ownership
            # (It body / Ta boundaries) is confirmed by the oracle tree during
            # admission, never assumed.
            opener = _xc_multiline(carrier, payload, suffix)
            if position == 'first':
                item = '.It Xo\n' + opener + f' Ta {XC_AUX_LEFT} Ta {XC_AUX_RIGHT}\n'
            elif position == 'middle':
                item = f'.It {XC_AUX_LEFT} Ta Xo\n' + opener + f' Ta {XC_AUX_RIGHT}\n'
            else:
                item = f'.It {XC_AUX_LEFT} Ta {XC_AUX_RIGHT} Ta Xo\n' + opener + '\n'
            body = '.Bl -column "' + 'x' * width + '" "xxxx" "xxxx"\n' + item + '.El\n'
        yield case('column-positions', 'mdoc', 'filled', body, width=width,
                   label=payload, suffix=suffix, layout=layout,
                   position=position, carrier=carrier)


def spacing_interval_cases():
    """XV: HEAD spacing requests versus vspace/skipvsp debt and BODY handoff."""
    for kind, width, head, (control_key, control), head_carrier, (body_key, body_line) \
            in itertools.product(N_KINDS, N_WIDTHS, XV_HEADS, XV_CONTROLS,
                                 XV_HEAD_CARRIERS, XV_BODY_CARRIERS):
        body = (f'.Bl -{kind} -width {width}n\n.It Xo\n'
                + operand('mdoc', head_carrier, head) + control
                + '.No AFTER\n.Xc\n' + body_line + '.El\n')
        yield case('spacing-intervals', 'mdoc', 'filled', body, kind=kind,
                   width=width, head=head, control=control_key,
                   head_carrier=head_carrier, body_carrier=body_key)


def spacing_reset_contrast_cases():
    """Named XV-derived contrasts: RESET replaced by non-word checkpoint states.

    Directed contrasts for the skipvsp transition; deliberately outside the
    frozen 1,152 XV count and registered separately in the ledger.
    """
    for variant, replacement in XV_RESET_VARIANTS:
        control = '.sp -1\n' + replacement + '.sp 1\n'
        body = ('.Bl -tag -width 4n\n.It Xo\n.No "D \\p E"\n' + control
                + '.No AFTER\n.Xc\n.No BodyWord\n.El\n')
        yield case('spacing-reset-contrasts', 'mdoc', 'filled', body,
                   variant=variant)


def _xe_expression(delimiter, truncated, payload, termination):
    if termination == 'complete':
        return delimiter + payload + delimiter
    if termination == 'missing-close':
        return delimiter + payload
    # Truncated closing: escaped delimiters cut their closing escape; literal
    # delimiters have no closing escape, so the input ends in an explicit
    # incomplete backslash and is registered as a recovery candidate.
    if truncated is None:
        return delimiter + payload + '\\'
    return delimiter + payload + truncated


def delimiter_argument_cases():
    """XE: escaped-delimited arguments, consumed tail bounds, recovery."""
    for (delim_key, delimiter, truncated), payload, prefix, carrier, context, termination \
            in itertools.product(XE_DELIMITERS, XE_PAYLOADS, XE_PREFIXES,
                                 ['No', 'Em', 'Lk'], ['filled', 'nf'],
                                 XE_TERMINATIONS):

        value = prefix + '\\o' + _xe_expression(delimiter, truncated, payload, termination) + ' Z'
        body = operand('mdoc', carrier, value) + '.No AFTER\n'
        yield case('delimiter-arguments', 'mdoc', context, body,
                   delimiter=delim_key, payload=payload, prefix=prefix,
                   carrier=carrier, termination=termination)


def repeated_link_identity_cases():
    """XI: two consecutive authored link occurrences and their identities."""
    for prefix, (control_key, control), label, (targets_key, first, second), context \
            in itertools.product(XW_PRE_STATES, XI_CONTROLS, XI_LABELS,
                                 XI_TARGET_PAIRS, XI_CONTEXTS):
        pre_line = f'.No "{prefix}"\n' if prefix else ''
        links = f'.Lk {first} "{label}"\n' + control + f'.Lk {second} "{label}"\n'
        if context == 'filled':
            body = pre_line + links + '.No AFTER\n'
        elif context == 'tag':
            body = ('.Bl -tag -width 8n\n.It Xo\n' + pre_line + links
                    + '.Xc\n.No BodyWord\n.El\n')
        else:
            body = ('.Bl -column "xxxx" "xxxx"\n.It Xo\n' + pre_line + links
                    + '.Xc Ta ' + XC_AUX_RIGHT + '\n.El\n')
        yield case('repeated-link-identities', 'mdoc', context, body,
                   prefix=prefix, control=control_key, label=label,
                   targets=targets_key)


def _xr_input_lines(dialect, context, empty_input):
    """Return (source_body_lines, input_line_numbers) for one XR case."""
    head_lines = HEAD[dialect].splitlines()
    open_lines, close_lines = [], []
    if context == 'nf':
        open_lines, close_lines = ['.nf\n'], ['.fi\n']
    elif context == 'definition':
        if dialect == 'man':
            open_lines = ['.TP 8\n', 'HeadWord\n']
        else:
            open_lines, close_lines = ['.Bl -tag -width 8n\n', '.It Xo\n'], ['.Xc\n']
    if empty_input == 'empty-source-line':
        input_lines = ['\n']
    elif empty_input == 'empty-operand':
        input_lines = ['.No ""\n' if dialect == 'mdoc' else '.B ""\n']
    elif empty_input == 'ampersand':
        input_lines = [r'\&' + '\n']
    else:
        input_lines = [r'\z' + '\n']
    offset = len(head_lines) + len(open_lines) + 1
    return open_lines, close_lines, input_lines, [offset]


def physical_row_handoff_cases():
    """XR: empty/zero-width inputs against physical row handoff at exits."""
    for dialect, context, empty_input, exit_kind, tail in itertools.product(
            ['man', 'mdoc'], ['filled', 'nf', 'definition'],
            XR_INPUTS, XR_EXITS, XR_TAILS):
        open_lines, close_lines, input_lines, input_source_lines = \
            _xr_input_lines(dialect, context, empty_input)
        inline_exit = ''
        section_exit = ''
        if exit_kind == 'br':
            inline_exit = '.br\n'
        elif exit_kind == 'sp1':
            inline_exit = '.sp 1\n'
        elif exit_kind == 'para':
            inline_exit = '.Pp\n' if dialect == 'mdoc' else '.PP\n'
        elif exit_kind == 'section':
            # A controlled heading ends the region; extraction never guesses
            # ranges from the trailing blank rows of the page.
            section_exit = ('.Sh EXITMARK\n.No EXITBODY\n' if dialect == 'mdoc'
                            else '.SH EXITMARK\nEXITBODY\n')
        after_line = '.No AFTER\n' if dialect == 'mdoc' else 'AFTER\n'
        next_lines = ['.Sh NEXT\n.No END\n'] if dialect == 'mdoc' else ['.SH NEXT\nEND\n']
        if context == 'definition' and dialect == 'mdoc':
            close_lines = close_lines + ['.El\n']
        body = ''.join(open_lines + input_lines
                       + ([inline_exit] if inline_exit else [])
                       + close_lines
                       + ([after_line] if tail == 'after' else [])
                       + ([section_exit] if section_exit else []))
        tail_lines = next_lines if tail == 'after' else []
        source = HEAD[dialect] + body + ''.join(tail_lines)
        yield dict(family='physical-row-handoffs', dialect=dialect,
                   context=context, empty_input=empty_input, exit=exit_kind,
                   tail=tail, input_source_lines=input_source_lines,
                   source=source)


EXPANSION_GENERATORS = [
    ('wrapper-entries', wrapper_entry_cases),
    ('cross-owner-words', cross_owner_word_cases),
    ('column-positions', column_position_cases),
    ('spacing-intervals', spacing_interval_cases),
    ('spacing-reset-contrasts', spacing_reset_contrast_cases),
    ('delimiter-arguments', delimiter_argument_cases),
    ('repeated-link-identities', repeated_link_identity_cases),
    ('physical-row-handoffs', physical_row_handoff_cases),
]

ADJACENT_GENERATORS = [
    ('cell-ownership', cell_ownership_cases),
    ('head-control-requests', head_control_request_cases),
    ('column-tail-rows', column_tail_row_cases),
]

# Section 5.6 frozen expansion counts (directed XV contrasts excluded).
EXPECTED_COUNTS = {
    'cell-ownership': 1344,
    'head-control-requests': 420,
    'column-tail-rows': 144,
    'wrapper-entries': 112,
    'cross-owner-words': 1152,
    'column-positions': 864,
    'spacing-intervals': 1152,
    'spacing-reset-contrasts': 3,
    'delimiter-arguments': 540,
    'repeated-link-identities': 288,
    'physical-row-handoffs': 240,
}


def families():
    """Yield (cohort, family, cases) with stable per-family ordering."""
    for family, generate in ADJACENT_GENERATORS:
        yield 'adjacent', family, generate()
    for family, generate in EXPANSION_GENERATORS:
        yield 'expansion', family, generate()


# --------------------------------------------------------------------------
# Assertion-axis policies (guide 5.7 vocabulary) as pure functions of axes
# --------------------------------------------------------------------------

_ROWS_BY_CONTEXT = {
    'filled': 'constrained-events: filled reflow, only authored breaks are hard rows',
    'nf': 'exact-hard-rows',
    'tag': 'exact-hard-rows',
    'hang': 'exact-hard-rows',
}


def _identity_policy(case):
    if 'Lk' in case.get('source', ''):
        return 'rich-inline'
    return 'display-and-safety'


def axis_policy(one_case):
    """Effective assertion-axis policy for one generated case."""
    family = one_case['family']
    context = one_case.get('context')
    rows_for_context = _ROWS_BY_CONTEXT.get(
        context, 'constrained-events: filled context, authored breaks only')
    policy = {
        'cell-ownership': {
            'content': 'exact',
            'separators': 'constrained-events',
            'rows': rows_for_context,
            'indent': 'omit-common-margin',
            'identity': _identity_policy(one_case),
        },
        'head-control-requests': {
            'content': 'exact',
            'separators': 'constrained-events',
            'rows': 'exact-hard-rows',
            'indent': 'responsive: list width places HEAD and BODY columns',
            'identity': _identity_policy(one_case),
        },
        'column-tail-rows': {
            'content': 'exact',
            'separators': 'not-applicable: column cell placement, not word separation',
            'rows': 'exact-hard-rows',
            'indent': 'responsive: column widths place cells',
            'identity': _identity_policy(one_case),
        },
        'wrapper-entries': {
            'content': 'exact',
            'separators': 'constrained-events',
            'rows': 'constrained-events: filled context, authored breaks only',
            'indent': 'omit-common-margin',
            'identity': 'rich-inline',
        },
        'cross-owner-words': {
            'content': 'exact',
            'separators': 'constrained-events',
            'rows': rows_for_context,
            'indent': 'omit-common-margin',
            'identity': _identity_policy(one_case),
        },
        'column-positions': {
            'content': 'exact',
            'separators': 'not-applicable: column cell placement, not word separation',
            'rows': 'exact-hard-rows',
            'indent': 'responsive: column widths place cells',
            'identity': _identity_policy(one_case),
        },
        'spacing-intervals': {
            'content': 'exact',
            'separators': 'constrained-events',
            'rows': 'exact-hard-rows',
            'indent': 'responsive: list width places HEAD and BODY columns',
            'identity': _identity_policy(one_case),
        },
        'spacing-reset-contrasts': {
            'content': 'exact',
            'separators': 'constrained-events',
            'rows': 'exact-hard-rows',
            'indent': 'responsive: list width places HEAD and BODY columns',
            'identity': 'display-and-safety',
        },
        'delimiter-arguments': {
            'content': 'exact',
            'separators': 'not-applicable: single delimited expression per operand',
            'rows': 'not-applicable: single word expression, no authored row break',
            'indent': 'omit-common-margin',
            'identity': _identity_policy(one_case),
        },
        'repeated-link-identities': {
            'content': 'exact',
            'separators': 'constrained-events',
            'rows': _ROWS_BY_CONTEXT.get(context, 'exact-hard-rows')
            if context in _ROWS_BY_CONTEXT else 'exact-hard-rows',
            'indent': 'responsive: column widths place cells' if context == 'column'
            else 'omit-common-margin',
            'identity': 'rich-inline',
        },
        'physical-row-handoffs': {
            'content': 'exact',
            'separators': 'constrained-events',
            'rows': 'exact-hard-rows',
            'indent': 'omit-common-margin',
            'identity': 'display-and-safety',
        },
    }[family]
    if family == 'delimiter-arguments' and one_case.get('termination') != 'complete':
        policy = dict(policy, content='recovery')
    if family == 'delimiter-arguments' and one_case.get('delimiter', '').startswith('literal') \
            and one_case.get('termination') == 'truncated-close':
        policy = dict(policy, content='recovery: trailing incomplete backslash candidate')
    return policy


# --------------------------------------------------------------------------
# Frozen selections: family representatives and directed RR-adjacent pairs
# --------------------------------------------------------------------------

# One representative per family (plus the three RR main anchors inside
# cell-ownership, which are members of that family) and, per repair axis, one
# positive plus one negative adjacent transition drawn from the expansion
# cohort.  These are the only cases whose full five-profile oracle record is
# checked in; everything else stays replay-only.
FROZEN_SELECTIONS = [
    ('rr01-anchor-a0080', 'cell-ownership',
     {'context': 'filled', 'prefix': r'\zP', 'word': r'Y\p\& \p Z', 'carrier': 'Lk'},
     'anchor', 'RR01 main example A0080: pending P, accepted label, AFTER'),
    ('rr01-anchor-a0212', 'cell-ownership',
     {'context': 'filled', 'prefix': r'\zP\z', 'word': r'\p  Y', 'carrier': 'Lk'},
     'anchor', 'RR01 main example A0212: rejected suffix must not revive'),
    ('rr05-anchor-a0030', 'cell-ownership',
     {'context': 'filled', 'prefix': '', 'word': r'\p\o\(aqXY\(aq Z', 'carrier': 'No'},
     'anchor', 'RR05 main example A0030: escaped delimiter leak'),
    ('representative-head-control-requests', 'head-control-requests',
     {'kind': 'tag', 'width': 8, 'label': 'LABEL', 'control': '.sp 1\n', 'carrier': 'No'},
     'representative', 'family representative'),
    ('representative-column-tail-rows', 'column-tail-rows',
     {'width': 8, 'label': r'D\p \p', 'suffix': ' No AFTER', 'layout': 'inline'},
     'representative', 'family representative'),
    ('representative-wrapper-entries', 'wrapper-entries',
     {'wrapper': 'Lk', 'prefix': r'\zP', 'post': 'none'},
     'representative', 'family representative'),
    ('representative-cross-owner-words', 'cross-owner-words',
     {'carrier': 'Lk', 'prefix': r'\zP', 'payload': r'Y\p\& \p Z',
      'context': 'tag', 'post': 'none'},
     'representative', 'family representative'),
    ('representative-column-positions', 'column-positions',
     {'width': 8, 'label': r'D\p \p', 'suffix': ' No AFTER', 'layout': 'multiline',
      'position': 'middle', 'carrier': 'No'},
     'representative', 'family representative'),
    ('representative-spacing-intervals', 'spacing-intervals',
     {'kind': 'tag', 'width': 4, 'head': r'D \p E', 'control': 'sp1-sp2',
      'head_carrier': 'No', 'body_carrier': 'no'},
     'representative', 'family representative (distinct request accumulation)'),
    ('representative-delimiter-arguments', 'delimiter-arguments',
     {'delimiter': 'literal-pipe', 'payload': r'X\zY', 'prefix': r'\p',
      'carrier': 'No', 'context': 'filled', 'termination': 'complete'},
     'representative', 'family representative'),
    ('representative-repeated-link-identities', 'repeated-link-identities',
     {'prefix': r'\zP', 'control': 'none', 'label': '', 'targets': 'same',
      'context': 'filled'},
     'representative', 'family representative'),
    ('representative-physical-row-handoffs', 'physical-row-handoffs',
     {'dialect': 'mdoc', 'context': 'definition', 'empty_input': 'empty-operand',
      'exit': 'none', 'tail': 'after'},
     'representative', 'family representative'),
    ('rr01-adjacent-positive', 'cross-owner-words',
     {'carrier': 'No', 'prefix': r'\zP', 'payload': 'Y', 'context': 'filled',
      'post': 'none'},
     'rr-adjacent-positive', 'W03: pending glyph P survives into the next real word'),
    ('rr01-adjacent-negative', 'cross-owner-words',
     {'carrier': 'No', 'prefix': r'\zP\z', 'payload': r'\p  Y', 'context': 'filled',
      'post': 'none'},
     'rr-adjacent-negative', 'W03: rejected suffix must stay rejected'),
    ('rr02-adjacent-positive', 'column-positions',
     {'width': 8, 'label': r'D\p \p', 'suffix': ' No AFTER', 'layout': 'inline',
      'position': 'first', 'carrier': 'No'},
     'rr-adjacent-positive', 'C01: authored row separates tested cell from later cells'),
    ('rr02-adjacent-negative', 'column-positions',
     {'width': 20, 'label': 'D', 'suffix': '', 'layout': 'inline',
      'position': 'first', 'carrier': 'No'},
     'rr-adjacent-negative', 'C01: un-wrapped row keeps all cells on one row'),
    ('rr03-adjacent-positive', 'spacing-intervals',
     {'kind': 'tag', 'width': 4, 'head': r'D \p E', 'control': 'sp1',
      'head_carrier': 'No', 'body_carrier': 'no'},
     'rr-adjacent-positive', 'V01: one explicit vspace is one blank row'),
    ('rr03-adjacent-negative', 'spacing-intervals',
     {'kind': 'tag', 'width': 4, 'head': r'D \p E', 'control': 'spneg1-sp1',
      'head_carrier': 'No', 'body_carrier': 'no'},
     'rr-adjacent-negative', 'V01: negative debt cancels the following request'),
    ('rr04-adjacent-positive', 'spacing-intervals',
     {'kind': 'hang', 'width': 4, 'head': r'D \p E', 'control': 'br',
      'head_carrier': 'No', 'body_carrier': 'no'},
     'rr-adjacent-positive', 'G01: AFTER and BodyWord keep a readable word boundary'),
    ('rr04-adjacent-negative', 'spacing-intervals',
     {'kind': 'hang', 'width': 4, 'head': r'D \p E', 'control': 'sp0',
      'head_carrier': 'No', 'body_carrier': 'no'},
     'rr-adjacent-negative', 'G01: zero-space run-in must not glue the words'),
    ('rr05-adjacent-positive', 'delimiter-arguments',
     {'delimiter': 'escape-aq', 'payload': 'XY', 'prefix': r'\p', 'carrier': 'No',
      'context': 'filled', 'termination': 'complete'},
     'rr-adjacent-positive', 'E01: escaped delimiter parses without control leak'),
    ('rr05-adjacent-negative', 'delimiter-arguments',
     {'delimiter': 'literal-apostrophe', 'payload': 'XY', 'prefix': r'\p',
      'carrier': 'No', 'context': 'filled', 'termination': 'complete'},
     'rr-adjacent-negative', 'E01: literal delimiter stays literal'),
] + [
    ('derived-reset-contrast-' + key, 'spacing-reset-contrasts', {'variant': key},
     'derived-contrast', 'XV-derived RESET replacement: ' + key)
    for key, _ in XV_RESET_VARIANTS
]

# Directed state transitions for the ledger coverage table.  Each row binds a
# transition to concrete case ids at replay time; consumers list what actually
# asserted it in this recording.
COVERAGE_TRANSITIONS = [
    {'transition': 'W03 pending glyph crosses owner entry (positive)',
     'selections': ['rr01-anchor-a0080', 'rr01-adjacent-positive'],
     'axes': ['content', 'separators', 'identity']},
    {'transition': 'W03 rejected suffix must not revive (negative)',
     'selections': ['rr01-anchor-a0212', 'rr01-adjacent-negative'],
     'axes': ['content', 'identity']},
    {'transition': 'C01 authored hard row separates column cells (positive)',
     'selections': ['rr02-adjacent-positive', 'representative-column-tail-rows'],
     'axes': ['rows', 'content']},
    {'transition': 'C01 width-only placement keeps one row (negative)',
     'selections': ['rr02-adjacent-negative'],
     'axes': ['rows', 'indent']},
    {'transition': 'V01 explicit vspace counts blank rows (positive)',
     'selections': ['rr03-adjacent-positive', 'representative-spacing-intervals'],
     'axes': ['rows', 'separators']},
    {'transition': 'V01 negative spacing debt cancels request (negative)',
     'selections': ['rr03-adjacent-negative'],
     'axes': ['rows']},
    {'transition': 'G01 head/body word boundary after control (positive)',
     'selections': ['rr04-adjacent-positive'],
     'axes': ['separators', 'rows']},
    {'transition': 'G01 zero-space run-in is no glue (negative)',
     'selections': ['rr04-adjacent-negative'],
     'axes': ['separators']},
    {'transition': 'E01 escaped delimiter tail bound (positive)',
     'selections': ['rr05-anchor-a0030', 'rr05-adjacent-positive'],
     'axes': ['content', 'identity']},
    {'transition': 'E01 literal delimiter stays literal (negative)',
     'selections': ['rr05-adjacent-negative'],
     'axes': ['content']},
    {'transition': 'W04/W05 wrapper entry without extra word execution',
     'selections': ['representative-wrapper-entries'],
     'axes': ['content', 'separators', 'identity']},
    {'transition': 'XI authored occurrences keep independent identities',
     'selections': ['representative-repeated-link-identities'],
     'axes': ['identity', 'content']},
    {'transition': 'XR empty/zero-width input physical row handoff',
     'selections': ['representative-physical-row-handoffs'],
     'axes': ['rows', 'content']},
    {'transition': 'XV-derived skipvsp reset by non-word states',
     'selections': ['derived-reset-contrast-wrapper-checkpoint',
                    'derived-reset-contrast-empty-operand',
                    'derived-reset-contrast-zero-width-word'],
     'axes': ['rows', 'separators']},
]

# Named witness (guide 5.6 XR): an existing checked-in fixture whose native
# tree deterministically contains an empty TEXT node, so empty-TEXT execution
# is never claimed from parser-normalized inputs that drop the node.
EMPTY_TEXT_WITNESS = {
    'fixture': 'formatter-1536',
    'matrix': 'crates/mant-engine/tests/roff_lowering/native_execution/fixtures/'
              'native_formatter_matrix.jsonl',
    'fact': 'Lk empty label keeps an empty TEXT node in the pristine tree',
}
