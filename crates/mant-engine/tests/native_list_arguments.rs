//! Native list normalization reaches geometry, JSON and original owner queries.
#[path = "support/native_source_contracts.rs"]
mod native_source_contracts;

#[test]
fn normalized_list_arguments_preserve_body_and_owner_contracts() {
    let fixture = include_str!("../../libmandoc-rs/tests/fixtures/list_arguments.json");
    for case in native_source_contracts::cases(fixture, 40) {
        native_source_contracts::assert_case(&case);
    }
}

#[test]
fn continued_head_and_literal_body_keep_exact_native_rows() {
    // The complete fixture ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // mdoc_term.c::print_mdoc_node checks NODE_LINE before dispatch except
    // after \c; termp_it_pre keeps HANG's field origin and trailing gap.
    // Native rows minus only the common five-cell page margin are exactly
    // those below. No author spaces or physical delimiters are folded.
    let source = include_str!("../../../tests/fixtures/roff/real/literal-definition-gap.1");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let restored: mant_ir::ResolvedContent = restored.into();
    let output = mant_render::render_query_man(&restored);
    // The string facade has no process terminator; the CLI adds its final LF.
    // The empty DESCRIPTION prefix and internal physical delimiter are content.
    assert_eq!(
        output.split_once("DESCRIPTION\n").unwrap().1,
        "\nX     1234567890    ABCDEFGHIJ    LAST\n      ROW2    SPACE"
    );
    assert_eq!(mant_render::render_query_text(&restored), output);
}
