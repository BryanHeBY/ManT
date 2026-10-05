//! Original portable table owner-range contracts.
use super::*;

#[test]
fn closed_cell_joins_rebase_existing_unicode_owner_ranges() {
    // Opaque borrowed keys are sufficient; no facts are dereferenced.
    let key = std::ptr::null();
    let output = join(
        [
            Projection::text("α\n\n".to_owned().into(), Tail::CompletedRows, true),
            Projection::text(
                MappedText {
                    text: "中BODY".into(),
                    owners: vec![(key, 0..7)],
                    ..Default::default()
                },
                Tail::Shared,
                true,
            ),
        ],
        " | ",
    )
    .finish();
    assert_eq!(output.text, "α\n\n中BODY");
    assert_eq!(output.owners, [(key, 4..11)]);
    assert_eq!(&output.text[output.owners[0].1.clone()], "中BODY");
}

#[test]
fn nested_literal_gap_closures_rebase_bytes_without_claiming_generated_rows() {
    let key = std::ptr::null();
    for depth in [1, 2, 4] {
        let mut first = Projection::text(
            MappedText {
                text: "α\n".into(),
                owners: vec![(key, 0..3)],
                ..Default::default()
            },
            Tail::Open,
            true,
        );
        first.gap(1);
        for _ in 0..depth {
            first = join([first], ", ");
        }
        let output = join(
            [
                first,
                Projection::text(
                    MappedText {
                        text: "中BODY".into(),
                        owners: vec![(key, 0..7)],
                        ..Default::default()
                    },
                    Tail::Shared,
                    true,
                ),
            ],
            " | ",
        )
        .finish();
        assert_eq!(output.text, "α\n\n\n中BODY");
        assert_eq!(output.owners, [(key, 0..3), (key, 5..12)]);
        assert_eq!(&output.text[output.owners[0].1.clone()], "α\n");
        assert_eq!(&output.text[output.owners[1].1.clone()], "中BODY");
    }
}
