"""Choose reproducible seeded samples and balance observed syntax coverage."""
from __future__ import annotations

import hashlib
from collections import Counter
from fractions import Fraction
from pathlib import Path
from typing import Iterable, Sequence

from .model import SyntaxProfile
from .sources import manual_section


def stable_sample(pages: Sequence[Path], maximum: int, seed: str) -> list[Path]:
    if maximum == 0 or len(pages) <= maximum:
        return list(pages)
    ranked = sorted(
        pages,
        key=lambda path: hashlib.sha256(
            f"{seed}\0{path.as_posix()}".encode("utf-8")
        ).digest(),
    )
    return sorted(ranked[:maximum], key=lambda path: path.as_posix())


def stable_sample_by_section(
    pages: Sequence[Path], maximum: int, seed: str
) -> list[Path]:
    grouped: dict[str, list[Path]] = {}
    for path in pages:
        section = manual_section(path)
        if section is not None:
            grouped.setdefault(section, []).append(path)
    sampled = [
        path
        for section in sorted(grouped)
        for path in stable_sample(grouped[section], maximum, f"{seed}\0{section}")
    ]
    return sorted(sampled, key=lambda path: path.as_posix())


def feature_counts(
    pages: Iterable[Path], profiles: dict[Path, SyntaxProfile]
) -> Counter[str]:
    return Counter(
        feature
        for page in pages
        for feature in profiles.get(
            page, SyntaxProfile(frozenset({"profile:error"}), 0)
        ).features
    )


def rare_feature_sample(
    pages: Sequence[Path],
    maximum: int,
    seed: str,
    profiles: dict[Path, SyntaxProfile],
    frequencies: Counter[str],
    coverage: Counter[str],
) -> list[Path]:
    if maximum == 0 or len(pages) <= maximum:
        selected = list(pages)
        coverage.update(feature_counts(selected, profiles))
        return selected
    remaining = set(pages)
    selected: list[Path] = []
    while remaining and len(selected) < maximum:
        def score(page: Path) -> tuple[Fraction, Fraction, bytes]:
            features = profiles[page].features
            uncovered = sum(
                (
                    Fraction(syntax_feature_weight(feature), frequencies[feature])
                    for feature in features
                    if not coverage[feature]
                ),
                start=Fraction(),
            )
            balance = sum(
                (
                    Fraction(
                        syntax_feature_weight(feature),
                        frequencies[feature] * (coverage[feature] + 1),
                    )
                    for feature in features
                ),
                start=Fraction(),
            )
            tie = hashlib.sha256(
                f"{seed}\0{page.as_posix()}".encode("utf-8")
            ).digest()
            return uncovered, balance, tie

        chosen = max(remaining, key=score)
        remaining.remove(chosen)
        selected.append(chosen)
        coverage.update(profiles[chosen].features)
    return sorted(selected, key=lambda path: path.as_posix())


def syntax_feature_weight(feature: str) -> int:
    return 3 if feature.startswith("interaction:") else 1


def rare_feature_sample_by_section(
    pages: Sequence[Path],
    maximum: int,
    seed: str,
    profiles: dict[Path, SyntaxProfile],
    frequencies: Counter[str],
    coverage: Counter[str],
) -> list[Path]:
    grouped: dict[str, list[Path]] = {}
    for page in pages:
        section = manual_section(page)
        if section is not None:
            grouped.setdefault(section, []).append(page)
    sampled = [
        page
        for section in sorted(grouped)
        for page in rare_feature_sample(
            grouped[section],
            maximum,
            f"{seed}\0{section}",
            profiles,
            frequencies,
            coverage,
        )
    ]
    return sorted(sampled, key=lambda path: path.as_posix())
