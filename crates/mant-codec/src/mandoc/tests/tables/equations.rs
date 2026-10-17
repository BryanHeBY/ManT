//! Inline equation payloads inside native tables.
use super::*;

#[test]
fn repeated_opaque_tbl_equations_never_start_isolated_parses() {
    let mut source =
        String::from(".TH TABLE-EQN-BUDGET 3\n.SH DESCRIPTION\n.EQ\ndelim %%\n.EN\n.TS\nl.\n");
    for index in 0..257 {
        writeln!(source, "%x{index}%").expect("write fixture row");
    }
    source.push_str(".TE\n");

    let document = parse_manual_bytes(
        std::path::Path::new("table-inline-equation-budget.3"),
        source.as_bytes(),
    )
    .expect("lower a bounded number of table equations");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected equation table");
    };
    assert_eq!(rows.len(), 257);
    // The pinned tbl tree still owns all 257 opaque cells. No guessed eqn
    // context can change them; their native payload stays complete.
    assert!(mant_ir::content_complete(&document.diagnostics));
    assert!(mant_ir::semantics_complete(&document.diagnostics));
    // CVS roff_parseln() bypasses inline eqn delimiters during tbl. The
    // exact 257-cell source ran ASCII/UTF8/HTML/tree/lint before this change.
    // Native payload stays complete; there is no speculative parse allowance.
    for (index, row) in rows.iter().enumerate() {
        let [Block::Paragraph { children, .. }] = row.cells[0].blocks.as_slice() else {
            panic!("native cell paragraph");
        };
        assert_eq!(inline_text(children), format!("%x{index}%"));
    }
    assert!(
        !document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.inline-equation-budget")
        })
    );
}
