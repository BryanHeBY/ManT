"""Declare a word/row matrix's frozen inputs and its specialized check together."""

from collections.abc import Callable, Iterator
from dataclasses import dataclass

from . import followup, pending_handoff, rows, section_padding, section_tail, sections, word
from .compare import check_rewind
from .core import core_sources
from .pending_observations import check_pending_rows
from .section_observations import check_section
from .section_padding_observations import check_section_padding
from .section_tail_observations import check_section_tail

BASELINE = "218edc22aa528651e824198708a6cbcb1e803ac8"
MEMBERS = {
    "A-atomic": 56232, "A-phase": 111456, "A-rearm": 91368, "A-seam": 60912,
    "B-core": 4320, "C-core": 2304,
    "A-route": 1080, "A-nested": 648, "A-spacing": 216, "A-sentence": 72,
    "A-retire": 288, "A-token-cut": 804, "A-blank": 256, "A-glyph": 144,
    "A-control": 192, "A-width": 192, "A-delivery": 144, "A-scale": 36,
    "bridge": 3456, "E-width": 18, "E-mode": 216, "E-space": 210,
    "E-node": 576, "E-owner": 240, "E-tail": 336, "E-dispatch": 120, "RS-review": 72,
}
FOLLOWUP_MEMBERS = {
    "W-core": 51840, "W-adapter": 320, "W-field": 96, "W-scale": 72,
    "ROW-hp-debt": 864, "ROW-hp-child": 324, "ROW-hp-scope": 144,
    "ROW-literal-owner": 600,
}
FOLLOWUP_BASELINE = "ee8ea0fa4931f84c983234b8fb0fd909457e7696"
SECTION_BASELINE = "d72a17e1cc63b9b6fdb90027c43a1258a9039d1e"
SECTION_MEMBERS = {
    "SEC-entry": 1344, "SEC-owner": 384, "SEC-owner-sh": 8,
    "SEC-chain": 8, "SEC-reset": 24, "SEC-display": 6, "SEC-pending": 84,
    "SEC-shared-control": 20,
}
PENDING_BASELINE = "9583fc7e98331d34f1e7435ed476bd3ee42a41f6"
PENDING_MEMBERS = {"PH-core": 432, "PH-br": 144, "PH-retained-br": 72}
SECTION_TAIL_BASELINE = "0424b8057132ffdc1a4050fcbf510f7825b9bae2"
SECTION_TAIL_MEMBERS = {
    "PHT-core": 512, "PHT-wrapped": 24, "PHT-head": 32, "PHT-boundary": 16,
    "PHT-chain": 12, "PHT-rewind": 4,
}
SECTION_PADDING_BASELINE = "d125616864f05e266f9fcf35f3e982a6d32f4c93"
SECTION_PADDING_MEMBERS = {
    "PHD-core": 1920, "PHD-ordered": 96, "PHD-wrapped": 72,
    "PHD-boundary": 36, "PHD-eof": 6, "PHD-source-event": 4, "PHD-scale": 15,
}


def records():
    generators = (
        core_sources, word.route_sources, word.spacing_sources, word.cut_sources,
        word.glyph_sources, word.delivery_sources, word.scale_sources, word.bridge_sources,
        rows.threshold_sources, rows.mode_sources, rows.space_sources, rows.node_sources,
        rows.owner_sources, rows.tail_sources, rows.dispatch_sources, rows.rs_sources,
    )
    for generate in generators:
        yield from generate()


def followup_records():
    for generate in (followup.rewind_sources, followup.bridge_sources, followup.scale_sources,
                     rows.hanging_post_sources, rows.literal_owner_sources):
        yield from generate()


def check_section_entry(_source, tree, receipt, observation):
    return check_section(tree, receipt, observation)


@dataclass(frozen=True)
class Lane:
    baseline: str
    members: dict[str, int]
    generate: Callable[[], Iterator[tuple[str, list, str]]]
    flag: str | None = None
    help: str = ""
    generator_files: tuple[str, ...] = ()
    observation: str | None = None
    check: Callable[..., dict] | None = None
    checked_cohorts: frozenset[str] | None = None
    verdict_files: tuple[str, ...] = ()

    def applies_to(self, cohorts):
        selected = self.members if self.checked_cohorts is None else self.checked_cohorts
        return self.check is not None and not cohorts.isdisjoint(selected)


CORE = Lane(BASELINE, MEMBERS, records)
SECTION_FACTS = ("section_facts.py", "section_observations.py")
LANES = (
    CORE,
    Lane(FOLLOWUP_BASELINE, FOLLOWUP_MEMBERS, followup_records,
         "--followup", "freeze only the word follow-up", ("followup.py",),
         "nativeWord", check_rewind, checked_cohorts=frozenset({"W-core"}),
         verdict_files=("followup.py",)),
    Lane(SECTION_BASELINE, SECTION_MEMBERS, sections.section_sources,
         "--sections", "freeze only the section follow-up", ("sections.py",),
         "nativeSection", check_section_entry, verdict_files=("section_observations.py",)),
    Lane(PENDING_BASELINE, PENDING_MEMBERS, pending_handoff.pending_sources,
         "--pending-rows", "freeze only the pending row handoff follow-up",
         ("pending_handoff.py",), "nativePendingRow", check_pending_rows,
         verdict_files=(*SECTION_FACTS, "pending_observations.py")),
    Lane(SECTION_TAIL_BASELINE, SECTION_TAIL_MEMBERS, section_tail.section_tail_sources,
         "--section-tail", "freeze only the ordered section-tail passes",
         ("pending_handoff.py", "section_tail.py"), "nativePendingRow", check_section_tail,
         verdict_files=(*SECTION_FACTS, "section_tail_observations.py")),
    Lane(SECTION_PADDING_BASELINE, SECTION_PADDING_MEMBERS, section_padding.padding_sources,
         "--section-padding", "freeze only the section-padding and completed-row controls",
         ("pending_handoff.py", "section_tail.py", "section_padding.py"),
         "nativePendingRow", check_section_padding,
         verdict_files=(*SECTION_FACTS, "section_padding.py", "section_padding_observations.py")),
)


def specialized_observations(source, tree, receipt, observation):
    """Apply each lane's check without overwriting a saved native obligation."""
    cohorts = set(source["cohorts"])
    applicable = [lane for lane in LANES if lane.applies_to(cohorts)]
    fields = [lane.observation for lane in applicable]
    if len(fields) != len(set(fields)):
        raise ValueError("source cohorts require conflicting native observations")
    return {lane.observation: lane.check(source["source"], tree, receipt, observation)
            for lane in applicable}
