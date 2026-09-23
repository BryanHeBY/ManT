//! Manual C05 scale probe; not an assertion about wall time or allocator RSS.

use super::*;
use std::fmt::Write as _;

#[test]
#[ignore = "run manually to compare native transfer and final IR at 1k/5k/20k rows"]
fn native_fixed_table_scale() {
    let rows = std::env::var("MANT_C05_ROWS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20_000);
    assert!([1_000, 5_000, 20_000].contains(&rows));
    // The exact generated inputs at all three sizes were run through fixed
    // CVS UTF-8/78. tbl_term.c::term_tbl owns the allbox geometry.
    let mut source = ".TH T 1\n.SH DATA\n.TS\nallbox tab(;);\nl l.\n".to_owned();
    for index in 0..rows {
        writeln!(source, "left_{index};right_{index}").expect("write to String");
    }
    source.push_str(".TE\n");
    let mut bundle = SourceBundle::new();
    bundle.insert("large-table.1", source.into_bytes()).unwrap();
    let start = std::time::Instant::now();
    if std::env::var("MANT_C05_PHASE").as_deref() == Ok("native") {
        let document =
            libmandoc_rs::structured::render_bundle("large-table.1", &bundle, InputFormat::Man)
                .expect("native structured transfer");
        eprintln!(
            "native rows={rows}, elapsed={:?}, atoms={}, fixed_lines={}, cells={}",
            start.elapsed(),
            document.content_atoms().len(),
            document.fixed_lines().len(),
            document.table_cells().len()
        );
        return;
    }
    let document = project_native_manual("large-table.1", &bundle, InputFormat::Man)
        .expect("native table reaches final IR");
    eprintln!(
        "final IR rows={rows}, elapsed={:?}, atoms={}, fixed_lines={}",
        start.elapsed(),
        document.content_store.atoms.len(),
        document.content_store.fixed_views[0].lines.len()
    );
}
