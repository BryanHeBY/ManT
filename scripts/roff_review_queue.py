"""Bounded review prioritization, never an acceptance or source-meaning oracle."""
from __future__ import annotations

from collections import Counter
import hashlib
import json
import re


REVIEW_FAMILY_SCHEMA = "mant.roff-review-family/v1"


def plain_output_controls(text: str) -> dict:
    """--color never/direct output cannot contain even otherwise valid SGR."""
    bad = Counter(c for c in text if (ord(c) < 32 and c not in '\n\t') or 127 <= ord(c) < 160)
    return {'status': 'hard-failure' if bad else 'covered',
            'characters': [{'codepoint': f'U+{ord(c):04X}', 'count': n} for c, n in sorted(bad.items())]}


def _difference_weight(comparison: dict) -> int:
    return sum(comparison.get('counts', {}).values())


def _count_bucket(count: int) -> str:
    """Describe a diff magnitude without turning a page-specific count into an ID."""
    if count <= 0:
        return "0"
    if count == 1:
        return "1"
    if count <= 4:
        return "2-4"
    if count <= 16:
        return "5-16"
    if count <= 64:
        return "17-64"
    return "65+"


def review_family(record: dict, residual: dict | None = None) -> dict:
    """Describe a repeatable review shape without weakening any raw evidence.

    Full-corpus renderer differences are often repeated terminal conventions
    rather than thousands of independent defects. This groups only the
    *observable audit evidence*: triage category, bounded difference-kind
    magnitudes, applied proof rules, and frame/geometry coverage. It neither
    interprets roff nor decides that two pages share a root cause. Every page
    keeps its own raw comparison, and every family remains reviewable.
    """
    residual = record.get("content", {}) if residual is None else residual
    triage = record.get("triage") or classify(record, residual)
    counts = residual.get("counts", {})
    traits = {
        "category": triage["category"],
        "residualStatus": residual.get("status", "uncovered"),
        "differenceBuckets": {
            kind: _count_bucket(int(count))
            for kind, count in sorted(counts.items())
            if isinstance(count, int) and count > 0
        },
        "explanationRules": sorted({
            str(explanation["rule"])
            for explanation in record.get("contentAssessment", {}).get("explanations", [])
            if isinstance(explanation, dict) and isinstance(explanation.get("rule"), str)
        }),
        "frameStatuses": {
            side: frame.get("status", "uncovered")
            for side, frame in sorted(record.get("frames", {}).items())
            if isinstance(frame, dict)
        },
        "geometryStatus": record.get("geometry", {}).get("status", "uncovered"),
        "rawControlStatus": record.get("rawControlCoverage", {}).get("status", "uncovered"),
    }
    encoded = json.dumps(traits, ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode()
    return {
        "schema": REVIEW_FAMILY_SCHEMA,
        "id": "family-" + hashlib.sha256(encoded).hexdigest()[:16],
        "traits": traits,
        "note": "Grouping aid only; member pages retain independent raw evidence and no family is accepted automatically.",
    }


def classify(record: dict, residual: dict | None = None) -> dict:
    """Order investigation; names describe signals, not confirmed defects."""
    residual = record.get('content', {}) if residual is None else residual
    counts = residual.get('counts', {})
    geometry = record.get('geometry', {})
    if record['status'] == 'hard-failure':
        category, priority = 'process-or-output-safety', 100
    elif record.get('reason'):
        category, priority = 'source-or-reference-coverage', 40
    elif any(finding.get('kind') == 'reference-control'
             for finding in record.get('content', {}).get('findings', [])):
        # A reference byte stream with cursor controls cannot establish a
        # trustworthy visible-text oracle. Keep it, but do not call it loss.
        category, priority = 'reference-output-coverage', 60
    elif counts.get('possible-control-operand'):
        category, priority = 'possible-operand-leak', 90
    elif counts.get('ambiguous-control-operand'):
        category, priority = 'ambiguous-operand-origin', 60
    elif any(frame.get('status') == 'partial'
             for frame in record.get('frames', {}).values()):
        # The raw content diff remains in evidence, but an incomplete header
        # or footer mask makes it unsafe to rank it as body-content loss.
        category, priority = 'frame-limited-content-coverage', 60
    elif (residual.get('status') == 'review'
          and record.get('contentAssessment', {}).get('explanations')
          and _difference_weight(residual) < _difference_weight(record.get('content', {}))):
        # A source-consistent secondary presentation lens removed part of the
        # raw divergence, but an actual residual remains. Keep it reviewable
        # without sending it to the same queue as unexplained whole-content
        # failures; raw counts and the residual are both preserved.
        category, priority = 'mixed-presentation-and-content-review', 70
    elif residual.get('status') == 'review':
        # Keep executable punctuation names in the higher tier. This ranking
        # does not authorize normalizing or discarding punctuation differences.
        significant = any(re.search(r'\w', token) or token.startswith(('-', '/'))
                          for finding in residual.get('findings', [])
                          for token in ([finding['token']] if 'token' in finding else
                                        finding.get('reference_preview', []) + finding.get('mant_preview', [])))
        category, priority = ('unexplained-content', 80) if significant else ('unexplained-presentation', 50)
    elif geometry.get('status') == 'review':
        category, priority = 'geometry-difference', 70
    elif residual.get('status') in ('partial', 'uncovered') or not residual.get('coverage', {}).get('complete', True):
        category, priority = 'content-coverage', 60
    elif record.get('content', {}).get('status') == 'review' and residual.get('status') == 'covered':
        category, priority = 'source-explained-presentation', 10
    elif record['status'] != 'clean':
        category, priority = 'other-coverage', 20
    else:
        category, priority = 'covered', 0
    return {'category': category, 'priority': priority,
            'confirmedProductDefect': False,
            'note': 'Investigation priority only; raw statuses and incomplete coverage remain authoritative.'}


class ArtifactSelection:
    """Keep strongest category/corpus representatives under page AND byte caps.

    No raw files are written until selection finishes, so rejected candidates
    cannot leave misleading artifact links or require destructive replacement.
    The memory cap includes raw streams and their serialized comparison report.
    """
    def __init__(self, pages: int, byte_limit: int = 64 * 1024 * 1024):
        if pages < 0 or byte_limit < 0:
            raise ValueError('artifact budgets must be nonnegative')
        self.pages, self.byte_limit = pages, byte_limit
        self.items = {}
        self.bytes = 0
        self.omitted = Counter()

    def consider(self, ordinal: int, record: dict, artifacts: dict[str, bytes]):
        if record['status'] == 'clean' or not artifacts:
            return
        if self.pages == 0:
            self.omitted['page-budget-disabled'] += 1
            return
        size = sum(len(data) for data in artifacts.values())
        if size > self.byte_limit:
            self.omitted['single-artifact-byte-budget'] += 1
            return
        identity = record['identities'][0]['id']
        corpus = identity.split(':', 1)[0] if ':' in identity else identity.rsplit('/', 1)[0]
        triage = record['triage']
        family = record.get("triageFamily", {})
        family_id = family.get("id", "unclassified") if isinstance(family, dict) else "unclassified"
        # Coverage categories alone group thousands of unrelated residuals.
        # Preserve a representative per observable evidence family and corpus;
        # this changes artifact selection only, never audit status or evidence.
        bucket = (triage['category'], family_id, corpus)
        # Prefer pages with real alignment/retention gaps within one bucket,
        # then occurrence count; neither makes them a confirmed product defect.
        content = record.get('content', {})
        score = (triage['priority'], not content.get('coverage', {}).get('complete', True),
                 min(10000, sum(content.get('counts', {}).values())), -ordinal)
        existing = self.items.get(bucket)
        if existing and existing['score'] >= score:
            self.omitted['represented-category-corpus'] += 1
            return
        candidates = {k: v for k, v in self.items.items() if k != bucket}
        used = sum(v['size'] for v in candidates.values())
        while candidates and (len(candidates) >= self.pages or used + size > self.byte_limit):
            weakest = min(candidates, key=lambda k: candidates[k]['score'])
            if candidates[weakest]['score'] >= score:
                self.omitted['lower-ranked-than-retained'] += 1
                return
            used -= candidates.pop(weakest)['size']
        if len(candidates) >= self.pages or used + size > self.byte_limit:
            self.omitted['artifact-budget'] += 1
            return
        candidates[bucket] = {'ordinal': ordinal, 'record': record, 'artifacts': artifacts,
                              'size': size, 'score': score}
        old_ordinals = {item['ordinal'] for item in self.items.values()}
        new_ordinals = {item['ordinal'] for item in candidates.values()}
        self.omitted['superseded-representative'] += len(old_ordinals - new_ordinals)
        self.items = candidates
        self.bytes = used + size

    def selected(self):
        return sorted(self.items.values(), key=lambda item: item['score'], reverse=True)
