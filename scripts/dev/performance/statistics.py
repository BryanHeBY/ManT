"""Paired differences, A/A noise and batch-preserving bootstrap statistics."""

import math
import random
import statistics

SEED = 20261002
RESAMPLES = 10_000


def finite_nonnegative(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and value >= 0


def quantile(values, fraction):
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    low = math.floor(position)
    high = math.ceil(position)
    return ordered[low] + (ordered[high] - ordered[low]) * (position - low)


def median_mad(values):
    middle = statistics.median(values)
    return {"median": middle, "mad": statistics.median(abs(value - middle) for value in values), "maximum": max(values)}


def paired_summary(batches, controls, *, seed=SEED, resamples=RESAMPLES):
    """Each pair is (baseline, candidate), independent of its execution order."""
    if not batches or any(not batch for batch in batches) or not controls:
        return {"status": "unmeasured", "reason": "missing batches or A/A controls"}
    if any(not finite_nonnegative(value) for group in [*batches, controls] for pair in group for value in pair):
        return {"status": "invalid", "reason": "non-finite or negative measurement"}
    differences = [[candidate - baseline for baseline, candidate in batch] for batch in batches]
    pairs = [pair for batch in batches for pair in batch]
    flat = [difference for batch in differences for difference in batch]
    randomizer = random.Random(seed)
    distribution = sorted(statistics.median([
        randomizer.choice(batch) for batch in differences for _ in range(len(batch))
    ]) for _ in range(resamples))
    interval = [quantile(distribution, 0.025), quantile(distribution, 0.975)]
    noise = quantile([abs(right - left) for left, right in controls], 0.95)
    median_difference = statistics.median(flat)
    batch_medians = [statistics.median(batch) for batch in differences]
    same_direction = all(value > 0 for value in batch_medians) or all(value < 0 for value in batch_medians)
    stable = same_direction and (interval[0] > 0 or interval[1] < 0) and abs(median_difference) > noise
    relative = [candidate / baseline - 1 if baseline else None for baseline, candidate in pairs]
    defined_relative = [value for value in relative if value is not None]
    return {
        "status": "stable" if stable else "noise-or-uncertain",
        "baseline": median_mad([pair[0] for pair in pairs]),
        "candidate": median_mad([pair[1] for pair in pairs]),
        "pairedDifferences": flat,
        "pairedRelativeDifferences": relative,
        "medianRelativeDifference": statistics.median(defined_relative) if defined_relative else None,
        "undefinedRelativeDifferences": len(relative) - len(defined_relative),
        "medianDifference": median_difference,
        "batchMedianDifferences": batch_medians,
        "bootstrap95": interval,
        "aaDifferences": [right - left for left, right in controls],
        "noiseP95AbsoluteDifference": noise,
        "method": {"seed": seed, "resamples": resamples, "resampling": "pairs within each batch, original batch size and weight"},
    }
