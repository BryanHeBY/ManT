use super::*;

#[test]
fn font_stack_divergence_is_pinned_in_native_html_and_lowered_ir() {
    // CVS term.c::term_fontlast()/term_fontpop() retain the previous explicit
    // selection across this mdoc scope pop.  GNU groff differs; ManT selects
    // the pinned CVS behavior, so both layers must keep TAIL bold.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh DESCRIPTION\n.No \\fBWORD\\fIINNER\n\\fPTAIL\n",
    );
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("font-stack.1", source.as_bytes())
        .expect("render the native font-stack contract")
        .output;
    assert!(native.contains("<b>TAIL</b>"), "native HTML: {native}");

    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower the font-stack case");
    let mut strong_tail = StrongText(false);
    strong_tail.visit_document(query.document.as_ref().expect("lowered document"));
    assert!(strong_tail.0, "lowered IR did not retain bold TAIL");
}
