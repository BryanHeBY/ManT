//! Linear composition of sorted, disjoint original-source decoration streams.

use std::ops::Range;

pub(crate) fn union(left: &[Range<usize>], right: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut left = left.iter().peekable();
    let mut right = right.iter().peekable();
    let mut result: Vec<Range<usize>> = Vec::new();
    while left.peek().is_some() || right.peek().is_some() {
        let next = if right.peek().is_none()
            || left
                .peek()
                .zip(right.peek())
                .is_some_and(|(a, b)| a.start <= b.start)
        {
            left.next().expect("remaining left range")
        } else {
            right.next().expect("remaining right range")
        };
        if let Some(last) = result.last_mut()
            && next.start <= last.end
        {
            last.end = last.end.max(next.end);
        } else {
            result.push(next.clone());
        }
    }
    result
}

pub(crate) fn overlay<T>(
    base: &mut Vec<T>,
    patches: Vec<T>,
    mut bounds: impl FnMut(&T) -> Range<usize>,
    mut slice: impl FnMut(&T, Range<usize>) -> T,
) {
    if patches.is_empty() {
        return;
    }
    let mut result = Vec::with_capacity(base.len() + patches.len());
    let mut originals = base.drain(..).peekable();
    for patch in patches {
        let range = bounds(&patch);
        while originals
            .peek()
            .is_some_and(|item| bounds(item).end <= range.start)
        {
            result.push(originals.next().expect("peeked base"));
        }
        if let Some(item) = originals.peek() {
            let original = bounds(item);
            if original.start < range.start {
                result.push(slice(item, original.start..range.start));
            }
        }
        while originals
            .peek()
            .is_some_and(|item| bounds(item).end <= range.end)
        {
            originals.next();
        }
        if let Some(item) = originals.peek_mut() {
            let original = bounds(item);
            if original.start < range.end {
                *item = slice(item, range.end..original.end);
            }
        }
        result.push(patch);
    }
    result.extend(originals);
    *base = result;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn protected_union_merges_touching_overlapping_and_nested_ranges() {
        let one = |range| std::iter::once(range).collect::<Vec<_>>();
        for (left, right, expected) in [
            (vec![], one(2..4), one(2..4)),
            (vec![0..2, 7..9], one(2..7), one(0..9)),
            (one(0..10), vec![2..4, 6..8], one(0..10)),
            (vec![0..2, 8..9], one(3..5), vec![0..2, 3..5, 8..9]),
        ] {
            assert_eq!(union(&left, &right), expected);
            assert_eq!(union(&right, &left), expected);
        }
    }

    #[test]
    fn patches_split_original_ranges_and_keep_gaps_and_payloads() {
        for base in [
            vec![],
            vec![(0..10, 1)],
            vec![(1..3, 1), (5..9, 2), (12..14, 3)],
        ] {
            for patches in [vec![], vec![(2..4, 4), (6..12, 5)], vec![(0..20, 6)]] {
                let mut actual = base.clone();
                overlay(
                    &mut actual,
                    patches.clone(),
                    |a| a.0.clone(),
                    |a, range| (range, a.1),
                );
                assert!(
                    actual
                        .windows(2)
                        .all(|pair| pair[0].0.end <= pair[1].0.start)
                );
                for byte in 0..24 {
                    let expected = patches
                        .iter()
                        .find(|a| a.0.contains(&byte))
                        .or_else(|| base.iter().find(|a| a.0.contains(&byte)))
                        .map(|a| a.1);
                    assert_eq!(
                        actual.iter().find(|a| a.0.contains(&byte)).map(|a| a.1),
                        expected
                    );
                }
            }
        }
    }

    #[test]
    fn many_patches_charge_only_linear_original_range_visits() {
        for size in [16, 256, 8192] {
            let mut base = vec![(0..size * 2, 1)];
            let patches = (0..size).map(|i| (2 * i..2 * i + 1, 2)).collect();
            let visits = Cell::new(0);
            overlay(
                &mut base,
                patches,
                |a| {
                    visits.set(visits.get() + 1);
                    a.0.clone()
                },
                |a, range| (range, a.1),
            );
            assert!(
                visits.get() <= 7 * size + 1,
                "{} visits for {size} patches",
                visits.get()
            );
            assert_eq!(base.len(), size * 2);
        }
    }
}
