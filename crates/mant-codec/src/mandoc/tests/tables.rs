//! Codec-internal lowering contracts; no query or rendering dependencies.
use super::*;

#[test]
fn bounds_distinct_tbl_equation_normalization_work() {
    let mut source =
        String::from(".TH TABLE-EQN-BUDGET 3\n.SH DESCRIPTION\n.EQ\ndelim %%\n.EN\n.TS\nl.\n");
    for index in 0..=MAX_INLINE_EQUATION_NORMALIZATIONS {
        writeln!(source, "%x{index}%").expect("write fixture row");
    }
    source.push_str(".TE\n");

    let document = parse_manual_bytes(
        std::path::Path::new("table-inline-equation-budget.3"),
        source.as_bytes(),
    )
    .expect("lower a bounded number of table equations");

    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.inline-equation-budget")
        })
    );
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected equation table");
    };
    assert_eq!(rows.len(), MAX_INLINE_EQUATION_NORMALIZATIONS + 1);
}
