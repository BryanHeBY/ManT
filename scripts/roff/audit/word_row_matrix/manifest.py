"""Freeze all logical coordinates and one source per exact byte hash."""
from collections import Counter
import gzip
import json
from pathlib import Path

from scripts.roff.fixtures.reference_recipes import digest
from .core import core_sources
from . import followup, rows, word

SCHEMA = "mant.word-row-matrix/v1"
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
EXTRA_WIDTHS = {"A-blank", "A-glyph", "A-width"}


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


def row_bytes(value):
    return (json.dumps(value, ensure_ascii=False, separators=(",", ":")) + "\n").encode()


def jsonl(path):
    with gzip.open(path, "rb") as stream:
        for line in stream:
            yield json.loads(line)


def freeze(directory, *, increment=False):
    directory.mkdir(parents=True, exist_ok=True)
    count = Counter()
    unique = {}
    member_index = []
    with gzip.GzipFile(directory / "members.jsonl.gz", "wb", compresslevel=3, mtime=0) as output:
        for cohort, coords, source in (followup_records() if increment else records()):
            source_hash = digest(source.encode())
            count[cohort] += 1
            identifier = cohort + "/" + row_bytes(coords).decode().removesuffix("\n")
            output.write(row_bytes(dict(id=identifier, cohort=cohort, coordinates=coords,
                                        sourceSha256=source_hash)))
            member_index.append((identifier, source_hash))
            record = unique.setdefault(source_hash, dict(sourceSha256=source_hash, source=source,
                                                          cohorts=set(), aliases=0))
            if record["source"] != source:
                raise ValueError("source hash collision")
            record["cohorts"].add(cohort)
            record["aliases"] += 1
    if count != (FOLLOWUP_MEMBERS if increment else MEMBERS):
        raise ValueError(f"cohort coordinates changed: {count!r}")
    member_index.sort()
    calibration = {source_hash for _, source_hash in member_index[::256]}
    with gzip.GzipFile(directory / "sources.jsonl.gz", "wb", compresslevel=3, mtime=0) as output:
        for source_hash, record in sorted(unique.items()):
            record["extraWidths"] = not EXTRA_WIDTHS.isdisjoint(record["cohorts"])
            record["cohorts"] = sorted(record["cohorts"])
            record["calibration"] = source_hash in calibration
            output.write(row_bytes(record))
    manifest = dict(
        schema=SCHEMA, baseline=FOLLOWUP_BASELINE if increment else BASELINE, members=sum(count.values()), cohorts=dict(count),
        sources=len(unique), aliases=sum(count.values()) - len(unique),
        wideRecipes=sum(record["extraWidths"] for record in unique.values()),
        calibrationSources=len(calibration),
        inputs={path.name: digest(path.read_bytes())
                for path in (directory / "members.jsonl.gz", directory / "sources.jsonl.gz")},
        generator={name: digest(Path(__file__).with_name(name).read_bytes())
                   for name in ("manifest.py", "core.py", "word.py", "rows.py",
                                *(("followup.py",) if increment else ()))},
    )
    manifest["nativeInvocations"] = 6 * manifest["sources"] + 5 * manifest["wideRecipes"]
    (directory / "manifest.json").write_bytes(row_bytes(manifest))
    return manifest
