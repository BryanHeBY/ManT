//! Public owner drains preserve native row facts and never expose field anchors.

#[test]
fn section_spacing_uses_native_body_children_even_without_visible_ir() {
    // Exact sources ran pristine CVS before these assertions. In
    // mdoc_term.c::termp_sh_pre(), the previous Sh body->child pointer,
    // rather than its rendered glyphs, controls the next term_vspace().
    for (body, expected) in [
        ("", 0),
        (".Dd September 28, 2026\n.Dt TEST 1\n.Os\n", 1),
        (".Sm off\n", 1),
        (".No X\n", 1),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh FIRST\n{body}.Sh NEXT\n.No END\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: mant_ir::ResolvedContent = decoded.into();
        assert_eq!(
            query.document.unwrap().sections[1].spacing_before_lines,
            expected,
            "{source}"
        );
    }
}
