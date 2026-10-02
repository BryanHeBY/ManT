"""Bounded occurrence matching for source-proved literal row boundaries.

The caller extracts supported source rows; this module does not execute roff or
infer rows from IR. Filled rows are ordered context, never hard-line obligations.
A repeated literal row cannot borrow an occurrence from another source owner.
"""
from bisect import bisect_left, bisect_right
from collections import defaultdict
from dataclasses import dataclass


@dataclass(frozen=True)
class SourceRow:
    line: int
    owner: int
    key: str
    literal: bool
    island: int


@dataclass(frozen=True)
class LineLimits:
    source_rows: int = 100_000
    rendered_tokens: int = 2_000_000
    occurrences_per_row: int = 512
    alignment_work: int = 24_000_000
    details: int = 256


class _Limit(Exception):
    pass


def _rendered(lines, limits):
    tokens, numbers, index = [], [], defaultdict(list)
    for row in lines:
        for word in row.key.split():
            if len(tokens) >= limits.rendered_tokens:
                raise _Limit('rendered-token-budget')
            index[word].append(len(tokens))
            tokens.append(word)
            numbers.append(row.number)
    return tokens, numbers, index


def _matches(words, rendered, limits, work):
    tokens, _, index = rendered
    positions = index.get(words[0], ())
    if len(positions) > limits.occurrences_per_row:
        return None
    matches = []
    for start in positions:
        work[0] += len(words)
        if work[0] > limits.alignment_work:
            raise _Limit('alignment-work-budget')
        end = start + len(words)
        if end <= len(tokens) and all(tokens[start + i] == word for i, word in enumerate(words)):
            matches.append((start, end))
    return matches


def _resolve(rows, words, choices):
    """Assign repeated rows between independently unique ordered landmarks.

    Every source row competes for its own occurrence, including filled context.
    Source line coordinates identify provenance, never collapse repetitions.
    The two linear landmark passes avoid a scan over all prior resolved rows.
    """
    resolved = {i: positions[0] for i, positions in enumerate(choices)
                if positions is not None and len(positions) == 1}
    collisions = defaultdict(list)
    for i, span in resolved.items():
        collisions[span].append(i)
    for indices in collisions.values():
        if len(indices) > 1:
            for i in indices:
                resolved.pop(i)
    before, after = [], [None] * len(rows)
    previous = None
    for i in range(len(rows)):
        before.append(previous)
        if i in resolved:
            previous = i
    following = None
    for i in reversed(range(len(rows))):
        after[i] = following
        if i in resolved:
            following = i
    groups = defaultdict(list)
    for i in range(len(rows)):
        if i not in resolved:
            groups[before[i], after[i], words[i]].append(i)
    for (left, right, _), indices in groups.items():
        positions = choices[indices[0]]
        if positions is None:
            continue
        low = resolved[left][1] if left is not None else 0
        high = resolved[right][0] if right is not None else float('inf')
        first = bisect_left(positions, (low, 0))
        last = len(positions) if high == float('inf') else bisect_right(positions, (high, 0))
        remaining = [span for span in positions[first:last] if span[1] <= high]
        if len(remaining) == len(indices):
            resolved.update(zip(indices, remaining))
    # Different row spellings can still overlap (e.g. a filled whole paragraph
    # and its literal prefix). Keep such assignments unresolved, never silently
    # claim that two owners consumed the same output cells.
    accepted, previous = {}, None
    for i, span in sorted(resolved.items()):
        if previous is not None and span[0] < previous[1][1]:
            accepted.pop(previous[0], None)
            previous = (i, span)
            continue
        accepted[i] = span
        previous = (i, span)
    return accepted


def _groups(rows, limits):
    """Coalesce each supported owner fragment before occurrence resolution.

    A filled paragraph may use a different source split than an identical
    literal block. Matching the full ordered fragment first keeps both owners
    distinguishable; literal physical row offsets remain separate witnesses.
    """
    groups, words, members = [], [], []
    token_count = 0
    for i, row in enumerate(rows):
        value = row.key.split()
        token_count += len(value)
        if token_count > limits.rendered_tokens:
            raise _Limit('source-token-budget')
        signature = row.owner, row.literal, row.island
        if not groups or signature != groups[-1]:
            groups.append(signature)
            words.append([])
            members.append([])
        members[-1].append((i, len(words[-1]), len(value)))
        words[-1].extend(value)
    return groups, [tuple(value) for value in words], members


def _row_spans(group_spans, members):
    output = {}
    for group, (start, _) in group_spans.items():
        for i, offset, length in members[group]:
            output[i] = start + offset, start + offset + length
    return output

def observe_hard_lines(source_rows, reference_lines, mant_lines, *, limits=None):
    """Return exact physical-line evidence and explicit bounded coverage gaps.

    Whole-line typography keys belong to the caller. Only token matching ignores
    soft wraps; the final decision compares physical row numbers without folding
    blank rows. No global set-membership or cross-owner exemption is used.
    """
    limits = limits or LineLimits()
    if any(value <= 0 for value in vars(limits).values()):
        raise ValueError('line observation budgets must be positive')
    report = {'schema': 'mant.roff-hard-line-observation/v1', 'status': 'not-applicable',
              'sourceRows': len(source_rows), 'literalPairs': 0, 'comparedPairs': 0,
              'mergedCount': 0, 'merged': [], 'gapDifferenceCount': 0, 'gapDifferences': [], 'compared': [], 'uncoveredCount': 0, 'uncovered': [],
              'alignmentWork': 0}
    work = [0]

    def detail(key, value):
        count = {'merged': 'mergedCount', 'gapDifferences': 'gapDifferenceCount', 'uncovered': 'uncoveredCount'}[key]
        report[count] += 1
        if len(report[key]) < limits.details:
            report[key].append(value)

    try:
        if len(source_rows) > limits.source_rows:
            raise _Limit('source-row-budget')
        pairs = [(i - 1, i) for i in range(1, len(source_rows))
                 if source_rows[i - 1].literal and source_rows[i].literal
                 and source_rows[i - 1].owner == source_rows[i].owner
                 and source_rows[i - 1].island == source_rows[i].island]
        report['literalPairs'] = len(pairs)
        if not pairs:
            return report
        groups, words, members = _groups(source_rows, limits)
        rendered = [_rendered(lines, limits) for lines in [reference_lines, mant_lines]]
        choices = [[_matches(value, output, limits, work) for value in words] for output in rendered]
        resolved = [_row_spans(_resolve(groups, words, value), members) for value in choices]
        for left, right in pairs:
            row = source_rows[right]
            provenance = {'sourceLines': [source_rows[left].line, row.line], 'owner': row.owner}
            if any(left not in mapping or right not in mapping for mapping in resolved):
                detail('uncovered', {'reason': 'ambiguous-or-unmatched-owner-occurrence', **provenance})
                continue
            spans = [(mapping[left], mapping[right]) for mapping in resolved]
            if any(second[0] != first[1] for first, second in spans):
                detail('uncovered', {'reason': 'unmodeled-content-between-source-rows', **provenance})
                continue
            rows = [[output[1][first[1] - 1], output[1][second[0]]]
                    for output, (first, second) in zip(rendered, spans)]
            if rows[0][1] <= rows[0][0]:
                detail('uncovered', {'reason': 'reference-does-not-retain-source-boundary', **provenance})
                continue
            report['comparedPairs'] += 1
            if len(report['compared']) < limits.details:
                report['compared'].append({**provenance, 'referenceLines': rows[0], 'mantLines': rows[1]})
            if rows[1][1] <= rows[1][0]:
                detail('merged', {**provenance, 'referenceLines': rows[0], 'mantLines': rows[1],
                                  'first': source_rows[left].key, 'second': row.key})
            elif rows[0][1] - rows[0][0] != rows[1][1] - rows[1][0]:
                # term_vspace produces actual completed rows; a repeated key
                # cannot discard them merely because unique-key lookup fails.
                # Compare the executed gap, never count source .sp requests.
                detail('gapDifferences', {**provenance, 'referenceLines': rows[0], 'mantLines': rows[1],
                                         'referenceBlankLines': rows[0][1] - rows[0][0] - 1,
                                         'mantBlankLines': rows[1][1] - rows[1][0] - 1})
    except _Limit as error:
        detail('uncovered', {'reason': str(error)})
    report['alignmentWork'] = work[0]
    report['status'] = ('review' if report['mergedCount'] or report['gapDifferenceCount'] else 'partial' if report['uncoveredCount']
                        else 'covered')
    return report
