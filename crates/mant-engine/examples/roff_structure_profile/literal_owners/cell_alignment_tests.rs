use super::*;

#[test]
fn bounded_witnesses_agree_with_enumerated_row_matches() {
    // Enumerate subsets on each side. Pairing them in order gives every
    // possible set of retained rows without relying on the edit recurrence.
    for source_len in 0..=4 {
        for source in 0..4_usize.pow(source_len) {
            let expected: Vec<_> = tokens(source, source_len)
                .enumerate()
                .map(|(at, token)| CellRow {
                    text: token.0.to_owned(),
                    owner: token.1.then_some(at / 2),
                })
                .collect();
            for observed_len in 0..=3 {
                for observed in 0..4_usize.pow(observed_len) {
                    let actual: Vec<_> = tokens(observed, observed_len).collect();
                    let result = edit_witnesses(&expected, &actual).unwrap();
                    let proof = enumerate_matches(&expected, &actual);
                    assert_eq!(result.total, proof.total, "{source}, {observed}");
                    assert_eq!(result.displays, proof.displays, "{source}, {observed}");
                }
            }
        }
    }
}

fn tokens(bits: usize, len: u32) -> impl Iterator<Item = (&'static str, bool)> {
    (0..len).map(move |index| {
        let token = (bits >> (2 * index)) & 3;
        (if token & 1 == 0 { "A" } else { "B" }, token & 2 != 0)
    })
}

fn enumerate_matches(expected: &[CellRow], actual: &[(&str, bool)]) -> EditWitnesses {
    let mut best = 0;
    let mut ranges = [None; 3];
    for kept in 0..1_usize << expected.len() {
        let count = kept.count_ones();
        if count < best || !can_match_subset(expected, actual, kept) {
            continue;
        }
        if count > best {
            best = count;
            ranges.fill(None);
        }
        let mut losses = [0; 3];
        for (at, row) in expected.iter().enumerate() {
            if let Some(owner) = row.owner
                && kept & (1 << at) == 0
            {
                losses[0] += 1;
                losses[owner + 1] += 1;
            }
        }
        for (range, loss) in ranges.iter_mut().zip(losses) {
            MissingRows::include(
                range,
                MissingRows {
                    min: loss,
                    max: loss,
                },
            );
        }
    }
    let [total, first, second] = ranges.map(Option::unwrap);
    EditWitnesses {
        total,
        displays: [first, second]
            .into_iter()
            .enumerate()
            .filter(|(_, loss)| loss.max > 0)
            .collect(),
    }
}

fn can_match_subset(expected: &[CellRow], actual: &[(&str, bool)], kept: usize) -> bool {
    let mut observations = actual.iter();
    expected
        .iter()
        .enumerate()
        .filter(|(at, _)| kept & (1 << at) != 0)
        .all(|(_, row)| {
            observations
                .any(|&(text, literal)| row.text == text && (literal || row.owner.is_none()))
        })
}
