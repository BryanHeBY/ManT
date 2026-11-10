"""Freeze all logical coordinates and one source per exact byte hash."""
from collections import Counter
import gzip
import json
from pathlib import Path

from scripts.roff.fixtures.reference_recipes import digest
from .lanes import CORE

SCHEMA = "mant.word-row-matrix/v1"
EXTRA_WIDTHS = {"A-blank", "A-glyph", "A-width"}


def row_bytes(value):
    return (json.dumps(value, ensure_ascii=False, separators=(",", ":")) + "\n").encode()


def jsonl(path):
    with gzip.open(path, "rb") as stream:
        for line in stream:
            yield json.loads(line)


def freeze(directory, *, lane=CORE):
    directory.mkdir(parents=True, exist_ok=True)
    count = Counter()
    unique = {}
    member_index = []
    with gzip.GzipFile(directory / "members.jsonl.gz", "wb", compresslevel=3, mtime=0) as output:
        for cohort, coords, source in lane.generate():
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
    if count != lane.members:
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
        schema=SCHEMA, baseline=lane.baseline, members=sum(count.values()), cohorts=dict(count),
        sources=len(unique), aliases=sum(count.values()) - len(unique),
        wideRecipes=sum(record["extraWidths"] for record in unique.values()),
        calibrationSources=len(calibration),
        inputs={path.name: digest(path.read_bytes())
                for path in (directory / "members.jsonl.gz", directory / "sources.jsonl.gz")},
        generator={name: digest(Path(__file__).with_name(name).read_bytes())
                   for name in ("manifest.py", "lanes.py", "core.py", "word.py", "rows.py",
                                *lane.generator_files)},
    )
    manifest["nativeInvocations"] = 6 * manifest["sources"] + 5 * manifest["wideRecipes"]
    (directory / "manifest.json").write_bytes(row_bytes(manifest))
    return manifest
