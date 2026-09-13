//! Cross-layer contracts for deliberate CVS and groff behavior choices.
//!
//! These cases are intentionally executed through both the vendored native
//! renderer and `ManT`'s lowering pipeline.  Updating the CVS snapshot must not
//! silently change one layer while leaving the other layer's expectation
//! frozen.  GNU groff is documented here as the secondary comparison, not run
//! by ordinary Cargo tests.

use libmandoc_rs::{RenderFormat, Renderer};
use mant_ir::{
    Inline, LinkTarget,
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
    native_contains: &'static [&'static str],
    lowered_contains: &'static [&'static str],
    selected: SelectedContract,
}

struct StrongText(bool);

struct AuthoredSectionLink {
    id: &'static str,
    found: bool,
}

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

impl<'ir> Visit<'ir> for AuthoredSectionLink {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if matches!(
            inline,
            Inline::Link {
                target: LinkTarget::Section { id },
                ..
            } if id.as_str() == self.id
        ) {
            self.found = true;
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
#[allow(clippy::too_many_lines)] // The table is intentionally one visible contract ledger.
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
            native_contains: &["     BEFOREAFTER\n     LAST"],
            lowered_contains: &["BEFOREAFTER\nLAST"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "enclosure close releases spacing but retains physical continuation",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.Eo [\n.No BEFORE\\c\n.Ec\n AFTER\n",
            ),
            native_contains: &["     [BEFORE  AFTER"],
            lowered_contains: &["[BEFORE  AFTER"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "Bx validator output is an executed generated word",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.Bx \\c\n.No AFTER\n",
            ),
            native_contains: &["     BSD AFTER"],
            lowered_contains: &["BSD AFTER"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "literal HP source continuation uses the portable joined projection",
            source: concat!(
                ".TH PROBE 1\n.SH DESCRIPTION\n.nf\n.HP 4\n",
                "FIRST\\c\nSECOND\nTHIRD\n.fi\nAFTER\n",
            ),
            native_contains: &["     FIRST\n         SECOND\n         THIRD"],
            lowered_contains: &["FIRSTSECOND\n    THIRD"],
            selected: SelectedContract::Groff,
        },
        TerminalCase {
            label: "CVS overstrike trims its trailing backspace blank pair",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.No A\\o'BX 'D\n",
            ),
            native_contains: &["     AXD"],
            lowered_contains: &["AXD"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "definition head flushes an occupied zero-advance cell",
            source: concat!(
                ".TH PROBE 1 \"September 13, 2026\"\n.SH NAME\nprobe \\- test\n",
                ".SH DESCRIPTION\n.TP\n.B A\\z\nBC\n",
            ),
            native_contains: &["     A", "BC"],
            lowered_contains: &["A", "BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "man section boundary flushes an occupied zero-advance cell",
            source: concat!(
                ".TH PROBE 1 \"September 13, 2026\"\n.SH NAME\nprobe \\- test\n",
                ".SH DESCRIPTION\nA\\z\n.SH NEXT\nBC\n",
            ),
            native_contains: &["\nNEXT\n", "BC"],
            lowered_contains: &["\nNEXT\n", "BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "bare zero-advance state is consumed by the next section heading",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No \\z\n",
                ".Sh NEXT SECTION\n.No AB\n.Sx NEXT SECTION\n",
            ),
            native_contains: &["\nEXT SECTION\n", "AB NEXT SECTION"],
            lowered_contains: &["\nEXT SECTION\n", "AB NEXT SECTION"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "Sm state survives a top-level section boundary",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n",
                ".Sm off\n.No one two\n.Sh NEXT\n.No three four\n",
            ),
            native_contains: &["onetwo", "threefour"],
            lowered_contains: &["onetwo", "threefour"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "Sm state survives a subsection boundary",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n",
                ".Sm off\n.No one two\n.Ss NEXT\n.No three four\n",
            ),
            native_contains: &["onetwo", "threefour"],
            lowered_contains: &["onetwo", "threefour"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "author mode requests in headings persist into the body",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh NAME\n.Nm probe\n.Nd test\n.Sh AUTHORS\n.An Root\n.An -nosplit\n",
                ".Sh TEST An -split\n.An Alice\n.An Bob\n",
            ),
            native_contains: &["     Alice\n     Bob"],
            lowered_contains: &["Alice\nBob"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "heading no-split mode replaces split before the body",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh NAME\n.Nm probe\n.Nd test\n.Sh AUTHORS\n.An Root\n.An -split\n",
                ".Sh TEST An -nosplit\n.An Alice\n.An Bob\n",
            ),
            native_contains: &["     Alice Bob"],
            lowered_contains: &["Alice Bob"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "heading author names execute split boundaries in source order",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.An -nosplit\n",
                ".Sh TEST An -split An Alice\n.An Bob\n",
            ),
            native_contains: &["\nTEST\nAlice\n", "\n     Bob"],
            lowered_contains: &["\nTEST\nAlice\nBob"],
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
        for expected in case.native_contains {
            assert!(
                native.contains(expected),
                "{} ({:?}) native output: {native:?}",
                case.label,
                case.selected,
            );
        }

        let lowered = lowered_terminal(case.source);
        for expected in case.lowered_contains {
            assert!(
                lowered.contains(expected),
                "{} ({:?}) lowered output: {lowered:?}",
                case.label,
                case.selected,
            );
        }
    }
}

#[test]
fn executed_heading_keeps_authored_navigation_identity() {
    // CVS mdoc_term.c executes the heading through term_word(), so a bare
    // BACKAFTER removes the first displayed character.  mdoc HTML still uses
    // the authored heading as the fragment identity; ManT likewise separates
    // executed presentation from navigation identity.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No \\z\n",
        ".Sh NEXT SECTION\n.No AB\n.Sx NEXT SECTION\n",
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower executed heading");
    let document = query.document.as_ref().expect("lowered document");
    let section = document
        .sections
        .iter()
        .find(|section| section.id.as_str() == "next-section")
        .expect("authored section identity");
    assert_eq!(section.heading.plain_text(), "EXT SECTION");

    let mut link = AuthoredSectionLink {
        id: "next-section",
        found: false,
    };
    link.visit_document(document);
    assert!(
        link.found,
        "authored .Sx did not resolve after heading execution"
    );
}

#[test]
fn executed_heading_display_cannot_shadow_an_authored_navigation_title() {
    // The pinned CVS formatter consumes the N in the first heading and shows
    // it as EXT, while its HTML target remains authored NEXT.  A later
    // authored EXT heading therefore remains the unique destination of
    // `.Sx EXT`; the projected display text is not a navigation alias.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No \\z\n",
        ".Sh NEXT\n.Sx EXT\n.Sh EXT\n.No BODY\n",
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower colliding headings");
    let document = query.document.as_ref().expect("lowered document");
    let headings = document
        .sections
        .iter()
        .map(|section| (section.id.as_str(), section.heading.plain_text()))
        .collect::<Vec<_>>();
    assert!(headings.contains(&("next", "EXT".to_owned())));
    assert!(headings.contains(&("ext", "EXT".to_owned())));

    let mut link = AuthoredSectionLink {
        id: "ext",
        found: false,
    };
    link.visit_document(document);
    assert!(link.found, "authored EXT reference was shadowed: {query:?}");
    assert!(
        document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("unresolved-section-reference")
        })
    );
}

#[test]
fn repeated_authored_section_titles_remain_ambiguous() {
    // CVS HTML resolves this to the first duplicate fragment.  ManT's stricter
    // navigation contract deliberately refuses to choose between two authored
    // destinations, while retaining both sections under unique stable IDs.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Sx DETAILS\n",
        ".Sh DETAILS\n.No ONE\n.Sh DETAILS\n.No TWO\n",
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower duplicate headings");
    let document = query.document.as_ref().expect("lowered document");
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.id == "details")
    );
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.id == "details-2")
    );

    for id in ["details", "details-2"] {
        let mut link = AuthoredSectionLink { id, found: false };
        link.visit_document(document);
        assert!(!link.found, "ambiguous authored title resolved to {id}");
    }
    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("unresolved-section-reference")
        })
    );
}

#[test]
fn mdoc_definition_styles_flush_occupied_zero_advance_heads() {
    // CVS mdoc_term.c::termp_it_post() calls term_newln() for tag, hang, and
    // overhang heads.  The occupied A cell therefore clears a trailing
    // BACKAFTER before the detached BC body executes.
    for style in ["tag", "hang", "ohang"] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width Ds\n.It No A\\z\nBC\n.El\n"
        );
        let native = native_terminal(&source);
        assert!(native.contains("BC"), "{style} native output: {native:?}");
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains("BC"),
            "{style} lowered output: {lowered:?}"
        );
        assert!(!lowered.contains("\nC"), "{style} lost B: {lowered:?}");
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
