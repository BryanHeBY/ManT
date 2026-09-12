//! Cross-layer contracts for deliberate CVS and groff behavior choices.
//!
//! These cases are intentionally executed through both the vendored native
//! renderer and `ManT`'s lowering pipeline.  Updating the CVS snapshot must not
//! silently change one layer while leaving the other layer's expectation
//! frozen.  GNU groff is documented here as the secondary comparison, not run
//! by ordinary Cargo tests.

use libmandoc_rs::{RenderFormat, Renderer};
use mant_ir::{
    Inline,
    visit::{self, Visit},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedContract {
    /// `ManT` deliberately follows the pinned CVS formatter instead of groff.
    Cvs,
    /// `ManT` deliberately retains the portable groff projection instead of a
    /// known CVS presentation artifact.
    Groff,
}

struct TerminalCase {
    label: &'static str,
    source: &'static str,
    native_contains: &'static str,
    lowered_contains: &'static str,
    selected: SelectedContract,
}

struct StrongText(bool);

impl<'ir> Visit<'ir> for StrongText {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Strong { children } = inline
            && super::inline_text(children) == "TAIL"
        {
            self.0 = true;
        }
        visit::walk_inline(self, inline);
    }
}

fn native_terminal(source: &str) -> String {
    let output = Renderer::new(RenderFormat::Utf8)
        .with_width(80)
        .render_bytes("contract.1", source.as_bytes())
        .expect("render the pinned native CVS contract")
        .output;
    apply_terminal_backspaces(&output)
}

fn apply_terminal_backspaces(output: &str) -> String {
    let mut projected = String::with_capacity(output.len());
    for character in output.chars() {
        if character == '\u{8}' {
            projected.pop();
        } else {
            projected.push(character);
        }
    }
    projected
}

fn lowered_terminal(source: &str) -> String {
    let query = mant_loader::load_roff_bytes(source.as_bytes())
        .expect("lower the same pinned CVS contract through ManT");
    mant_render::render_query_text(&query)
}

#[test]
fn terminal_divergence_matrix_pins_native_and_lowered_behavior_together() {
    // Expectations were first reproduced with the unpatched pinned reference
    // binary.  The execution rules come from CVS term.c (ESCAPE_BREAK,
    // ESCAPE_NOSPACE, BACKAFTER/BACKBEFORE and ESCAPE_OVERSTRIKE),
    // mdoc_validate.c::post_bx(), and man_term.c::pre_HP().
    let cases = [
        TerminalCase {
            label: "word-end break followed by no-space",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.No BEFORE\\p\\c\n.No AFTER LAST\n",
            ),
            native_contains: "     BEFOREAFTER\n     LAST",
            lowered_contains: "BEFOREAFTER\nLAST",
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "enclosure close releases spacing but retains physical continuation",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.Eo [\n.No BEFORE\\c\n.Ec\n AFTER\n",
            ),
            native_contains: "     [BEFORE  AFTER",
            lowered_contains: "[BEFORE  AFTER",
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "Bx validator output is an executed generated word",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.Bx \\c\n.No AFTER\n",
            ),
            native_contains: "     BSD AFTER",
            lowered_contains: "BSD AFTER",
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "literal HP source continuation uses the portable joined projection",
            source: concat!(
                ".TH PROBE 1\n.SH DESCRIPTION\n.nf\n.HP 4\n",
                "FIRST\\c\nSECOND\nTHIRD\n.fi\nAFTER\n",
            ),
            native_contains: "     FIRST\n         SECOND\n         THIRD",
            lowered_contains: "FIRSTSECOND\n    THIRD",
            selected: SelectedContract::Groff,
        },
        TerminalCase {
            label: "CVS overstrike trims its trailing backspace blank pair",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.No A\\o'BX 'D\n",
            ),
            native_contains: "     AXD",
            lowered_contains: "AXD",
            selected: SelectedContract::Cvs,
        },
    ];

    assert!(
        cases
            .iter()
            .any(|case| case.selected == SelectedContract::Groff)
    );
    for case in cases {
        let native = native_terminal(case.source);
        assert!(
            native.contains(case.native_contains),
            "{} ({:?}) native output: {native:?}",
            case.label,
            case.selected,
        );

        let lowered = lowered_terminal(case.source);
        assert!(
            lowered.contains(case.lowered_contains),
            "{} ({:?}) lowered output: {lowered:?}",
            case.label,
            case.selected,
        );
    }
}

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
