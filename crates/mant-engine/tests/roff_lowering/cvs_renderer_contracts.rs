//! Cross-layer contracts for deliberate CVS and groff behavior choices.
//!
//! These cases are intentionally executed through both the vendored native
//! renderer and `ManT`'s lowering pipeline.  Updating the CVS snapshot must not
//! silently change one layer while leaving the other layer's expectation
//! frozen.  GNU groff is documented here as the secondary comparison, not run
//! by ordinary Cargo tests.

use libmandoc_rs::{RenderFormat, Renderer};
use mant_ir::{
    Block, DefinitionItem, Document, Inline, LinkTarget,
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

struct ExactTailInline(Option<Inline>);

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

impl<'ir> Visit<'ir> for ExactTailInline {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if self.0.is_none() && mant_ir::inline_plain_text(std::slice::from_ref(inline)) == "TAIL" {
            self.0 = Some(inline.clone());
            return;
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
    apply_terminal_backspaces(&native_terminal_raw(source))
}

fn native_terminal_raw(source: &str) -> String {
    Renderer::new(RenderFormat::Utf8)
        .with_width(80)
        .render_bytes("contract.1", source.as_bytes())
        .expect("render the pinned native CVS contract")
        .output
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

fn without_line_indentation(output: &str) -> String {
    output
        .lines()
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_definition_item(document: &Document) -> &DefinitionItem {
    document
        .sections
        .iter()
        .flat_map(|section| section.blocks.iter())
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => items.first(),
            _ => None,
        })
        .expect("definition item")
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
            label: "Sx display spacing does not change its authored destination",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh NEXT SECTION\n.No FIRST\n.Sh NEXTSECTION\n.No SECOND\n",
                ".Sh SEE ALSO\n.Sm off\n.Sx NEXT SECTION\n",
            ),
            native_contains: &["     NEXTSECTION"],
            lowered_contains: &["NEXTSECTION"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "Fl sees an adjacent An while heading state executes in order",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.Sh Fl An Alice\n.No BODY\n",
            ),
            native_contains: &["-Alice"],
            lowered_contains: &["-Alice"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "Pf sees an adjacent An while heading state executes in order",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.Sh Pf $ An Alice\n.No BODY\n",
            ),
            native_contains: &["$Alice"],
            lowered_contains: &["$Alice"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "control-only An preserves Fl adjacency in a heading",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.Sh Fl An -split Cm name\n.No BODY\n",
            ),
            native_contains: &["-name"],
            lowered_contains: &["-name"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "nested An executes at its recursive formatter position",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh DESCRIPTION\n.Sh Dq An -split An Alice\n.No BODY\n",
            ),
            native_contains: &["“\nAlice”"],
            lowered_contains: &["“\nAlice”"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "inset head and generated body gap share zero-advance state",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".Bl -inset\n.It A\\zX\n.No BC\n.El\n",
            ),
            native_contains: &["     A\u{a0}BC"],
            lowered_contains: &["A BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "bare zero-advance consumes an inset body gap",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".Bl -inset\n.It A\\z\n.No BC\n.El\n",
            ),
            native_contains: &["     ABC"],
            lowered_contains: &["ABC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "diagnostic head and two generated cells share zero-advance state",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".Bl -diag\n.It A\\zX\n.No BC\n.El\n",
            ),
            native_contains: &["     A\u{a0}\u{a0}BC"],
            lowered_contains: &["A  BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "indented inset body starts a new native formatter row",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".Bl -inset\n.It A\n BC\n.El\n",
            ),
            native_contains: &["     A\u{a0}\n      BC"],
            lowered_contains: &["A \n BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "transparent target preserves an indented diagnostic body row",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".Bl -diag\n.It A\n.Tg mark\n BC\n.El\n",
            ),
            native_contains: &["     A\u{a0}\u{a0}\n      BC"],
            lowered_contains: &["A  \n BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "macro-expanded inset uses NODE_LINE rather than source coordinates",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".de XX\n.Bl -inset\n.It A\\zX\n BC\n.El\n..\n.XX\n",
            ),
            native_contains: &["\n      BC"],
            lowered_contains: &["A \n BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "macro-expanded diagnostic uses NODE_LINE rather than source coordinates",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".de XX\n.Bl -diag\n.It A\\zX\n BC\n.El\n..\n.XX\n",
            ),
            native_contains: &["\n      BC"],
            lowered_contains: &["A  \n BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "empty diagnostic head still executes its generated body cells",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".Bl -diag\n.It\n BC\n.El\n",
            ),
            native_contains: &["     \u{a0}\u{a0}\n      BC"],
            lowered_contains: &["  \n BC"],
            selected: SelectedContract::Cvs,
        },
        TerminalCase {
            label: "macro-expanded empty diagnostic head retains its generated row",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
                ".de XX\n.Bl -diag\n.It\n BC\n.El\n..\n.XX\n",
            ),
            native_contains: &["     \u{a0}\u{a0}\n      BC"],
            lowered_contains: &["  \n BC"],
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
            label: "overrun hang field keeps source content across a margin flush",
            source: concat!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
                ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n",
                ".Bl -hang -width 6n\n.It Xo\n.No LONGTEXT\n.mc\n.No A\n",
                ".An -split\n.An Bob\n.Xc\n.No BODY\n.El\n",
            ),
            // CVS loses A when the overrun HANG field is committed under
            // NOBREAK. GNU groff retains it; ManT selects the content-safe
            // projection rather than reproducing that terminal artifact.
            native_contains: &["     LONGTEXT Bob BODY"],
            lowered_contains: &["LONGTEXT A Bob BODY"],
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
fn sx_display_state_cannot_change_its_authored_destination() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NEXT SECTION\n.No FIRST\n.Sh NEXTSECTION\n.No SECOND\n",
        ".Sh SEE ALSO\n.Sm off\n.Sx NEXT SECTION\n",
    );
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("sx-authored.1", source.as_bytes())
        .expect("render native Sx identity")
        .output;
    assert!(
        native.contains("href=\"#NEXT_SECTION\">NEXTSECTION</a>"),
        "native HTML: {native}"
    );

    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower Sx identity");
    let document = query.document.as_ref().expect("lowered document");
    let mut correct = AuthoredSectionLink {
        id: "next-section",
        found: false,
    };
    correct.visit_document(document);
    assert!(correct.found, "Sx target was inferred from display text");
}

#[test]
fn section_and_sx_share_cvs_deroff_authored_normalization() {
    for (heading, reference) in [("\\&NEXT", "NEXT"), ("NEXT", "\\&NEXT")] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh {heading}\n.No BODY\n.Sh SEE ALSO\n.Sx {reference}\n"
        );
        let native = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render_bytes("sx-deroff.1", source.as_bytes())
            .expect("render native deroff identity")
            .output;
        assert!(
            native.contains("class=\"Sx\" href=\"#NEXT\""),
            "native HTML: {native}"
        );

        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower deroff identity");
        let document = query.document.as_ref().expect("lowered document");
        let mut link = AuthoredSectionLink {
            id: "next",
            found: false,
        };
        link.visit_document(document);
        assert!(link.found, "authored target did not normalize: {query:?}");
        assert!(document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("unresolved-section-reference")
        }));
    }
}

#[test]
fn sx_authored_escape_cannot_collapse_into_a_display_equivalent_heading() {
    // html.c::html_make_id() derives both Sh and Sx fragments from the
    // authored text before terminal `\z` projection.  The two visually equal
    // headings therefore remain distinct navigation identities.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh AC\n.No PLAIN\n.Sh A\\zBC\n.No ESCAPED\n",
        ".Sh SEE ALSO\n.Sx A\\zBC\n",
    );
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("sx-authored-escape.1", source.as_bytes())
        .expect("render native escape-bearing Sx identity")
        .output;
    assert!(native.contains("id=\"A_zBC\""), "native HTML: {native}");
    assert!(
        native.contains("href=\"#A_zBC\">AC</a>"),
        "native HTML: {native}"
    );

    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower authored Sx escape");
    let document = query.document.as_ref().expect("lowered document");
    assert!(document.sections.iter().any(|section| section.id == "ac"));
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.id == "a-zbc")
    );
    let mut correct = AuthoredSectionLink {
        id: "a-zbc",
        found: false,
    };
    correct.visit_document(document);
    assert!(correct.found, "Sx target was inferred from projected AC");
}

#[test]
fn nested_heading_author_modes_execute_without_losing_inline_adjacency() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh Dq An -split An Alice\n.No BODY\n",
    );
    let native = native_terminal(source);
    assert!(native.contains("“\nAlice”"), "native terminal: {native:?}");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower nested An heading");
    let heading = &query.document.as_ref().unwrap().sections[1].heading.content;
    assert!(
        heading
            .iter()
            .any(|inline| matches!(inline, Inline::LineBreak)),
        "nested An split was not executed: {heading:?}"
    );
    assert_eq!(super::inline_text(heading), "“\nAlice”");
}

#[test]
fn heading_wrappers_remove_only_the_structural_bold_layer() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh TARGET\n.No BODY\n.Sh Sx TARGET\n.No SX\n",
        ".Sh Lk https://example.org Label\n.No LK\n",
    );
    let native = native_terminal_raw(source);
    assert!(
        native.contains("_\u{8}T\u{8}T"),
        "CVS heading did not combine bold and underline: {native:?}"
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower linked headings");
    let markdown = mant_codec::encode::render_markdown(&query);
    assert!(markdown.contains("## *TARGET*"), "{markdown}");
    assert!(
        markdown.contains("## [*Label*](https://example.org)"),
        "{markdown}"
    );
    assert!(!markdown.contains("***TARGET***"), "{markdown}");
    assert!(!markdown.contains("***Label***"), "{markdown}");
}

#[test]
fn run_in_definition_handoff_preserves_native_gap_and_source_row_contracts() {
    let cases = [
        ("inset", "", "BC", ""),
        ("inset", "\\&", "BC", ""),
        ("inset", "A", " BC", "A"),
        ("diag", "A", " BC", "A"),
        ("diag", "", " BC", ""),
    ];
    for (style, head, body, term) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style}\n.It {head}\n{body}\n.El\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower run-in item");
        let item = first_definition_item(query.document.as_ref().unwrap());
        let term_text = item
            .terms
            .iter()
            .map(|term| super::inline_text(term))
            .collect::<String>();
        assert_eq!(term_text, term, "{style} {head:?}: {item:?}");
        let description = item
            .description
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => Some(super::inline_text(children)),
                _ => None,
            })
            .unwrap();
        let expected = match (style, head, body.starts_with(' ')) {
            ("inset", "", _) => "BC",
            ("inset", "\\&", _) => " BC",
            ("inset", _, true) => " \n BC",
            ("diag", _, true) => "  \n BC",
            _ => unreachable!(),
        };
        assert_eq!(description, expected, "{style} {head:?}: {item:?}");
        assert!(item.layout.inline_term, "{style} {head:?}: {item:?}");
        assert_eq!(item.layout.min_term_gap_columns, 0);
    }
}

#[test]
fn transparent_target_does_not_reset_a_run_in_formatter_boundary() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
        ".Bl -diag\n.It A\n.Tg mark\n BC\n.El\n",
    );
    let native = native_terminal(source);
    assert!(native.contains("A\u{a0}\u{a0}\n      BC"), "{native:?}");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower target run-in");
    let item = first_definition_item(query.document.as_ref().unwrap());
    let description = item
        .description
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. } => Some(super::inline_text(children)),
            _ => None,
        })
        .unwrap();
    assert_eq!(description, "  \n BC");
    assert!(
        serde_json::to_string(item).unwrap().contains("mark"),
        "target was not attached to its run-in owner: {item:?}"
    );
}

#[test]
fn inherited_zero_advance_cannot_change_an_sx_destination() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh ABC\n.No FIRST\n.Sh BC\n.No SECOND\n",
        ".Sh SEE ALSO\n.No \\z\n.Sx ABC\n",
    );
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("sx-zero.1", source.as_bytes())
        .expect("render native Sx zero-advance identity")
        .output;
    assert!(
        native.contains("href=\"#ABC\""),
        "native HTML lost authored target: {native}"
    );

    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower Sx zero identity");
    let document = query.document.as_ref().expect("lowered document");
    let mut correct = AuthoredSectionLink {
        id: "abc",
        found: false,
    };
    correct.visit_document(document);
    assert!(correct.found, "display BC incorrectly selected section BC");
}

#[test]
fn heading_and_diagnostic_scopes_preserve_previous_font_execution() {
    let cases = [
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
            ".Sh NEXT\\fI\n\\fPTAIL\n",
        ),
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
            ".Ss NEXT\\fI\n\\fPTAIL\n",
        ),
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
            ".Bl -diag\n.It A\\fI\n\\fPTAIL\n.El\n",
        ),
    ];
    for source in cases {
        let native = native_terminal_raw(source);
        assert!(
            native.contains("T\u{8}TA\u{8}AI\u{8}IL\u{8}L"),
            "native terminal did not retain bold TAIL: {native:?}"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower font scope");
        let mut strong_tail = StrongText(false);
        strong_tail.visit_document(query.document.as_ref().expect("lowered document"));
        assert!(strong_tail.0, "lowered IR did not retain bold TAIL");
    }

    // mdoc_term.c applies the diagnostic bold scope in termp_it_pre(), but
    // inset has no such scope.  term_fontpopq() restores the current stack
    // while leaving the previous-font register updated for a later `\fP`.
    let inset = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
        ".Bl -inset\n.It A\\fI\n\\fPTAIL\n.El\n",
    );
    let native = native_terminal_raw(inset);
    assert!(
        !native.contains("T\u{8}TA\u{8}AI\u{8}IL\u{8}L"),
        "inset made TAIL bold: {native:?}"
    );
    let query = mant_loader::load_roff_bytes(inset.as_bytes()).expect("lower inset font scope");
    let mut strong_tail = StrongText(false);
    strong_tail.visit_document(query.document.as_ref().expect("lowered document"));
    assert!(!strong_tail.0, "inset inherited diagnostic bold scope");
}

#[test]
fn man_and_mdoc_headings_keep_distinct_previous_font_contracts() {
    for heading in ["SH", "SS"] {
        let source = format!(
            ".TH PROBE 1 \"September 13, 2026\"\n.SH NAME\nprobe \\- test\n.{heading} NEXT\\fI\n\\fPTAIL\n"
        );
        let native = native_terminal_raw(&source);
        assert!(native.contains("TAIL"), "man {heading}: {native:?}");
        assert!(
            !native.contains("T\u{8}TA\u{8}AI\u{8}IL\u{8}L"),
            "man {heading} left TAIL bold: {native:?}"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower man heading");
        let mut tail = ExactTailInline(None);
        tail.visit_document(query.document.as_ref().expect("lowered document"));
        assert_eq!(
            tail.0,
            Some(Inline::Text {
                value: "TAIL".to_owned()
            }),
            "man {heading} retained the wrong font state"
        );
    }

    for heading in ["Sh", "Ss"] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.{heading} NEXT\\fI\n\\fPTAIL\n"
        );
        let native = native_terminal_raw(&source);
        assert!(
            native.contains("T\u{8}TA\u{8}AI\u{8}IL\u{8}L"),
            "mdoc {heading} lost bold TAIL: {native:?}"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower mdoc heading");
        let mut tail = ExactTailInline(None);
        tail.visit_document(query.document.as_ref().expect("lowered document"));
        assert_eq!(
            tail.0,
            Some(Inline::Strong {
                children: vec![Inline::Text {
                    value: "TAIL".to_owned()
                }]
            }),
            "mdoc {heading} lost the exact previous-font state"
        );
    }
}

#[test]
fn definition_heads_execute_author_modes_in_native_node_order() {
    let cases = [
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
            ".Sh DESCRIPTION\n.Bl -inset\n.It Xo An -split An Alice An Bob Xc\n",
            ".No BODY\n.El\n.No END\n",
        ),
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
            ".Sh DESCRIPTION\n.Bl -inset\n.It Xo\n.Ao\n.An -split\n.An Alice\n",
            ".An Bob\n.Ac\n.Xc\n.No BODY\n.El\n.No END\n",
        ),
    ];
    for source in cases {
        let native = native_terminal(source);
        assert!(
            native.contains("Alice\n"),
            "native author split: {native:?}"
        );
        assert!(native.contains("Bob"), "native author output: {native:?}");

        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower author head");
        let item = first_definition_item(query.document.as_ref().unwrap());
        let terms = item
            .terms
            .iter()
            .map(|term| super::inline_text(term))
            .collect::<Vec<_>>();
        assert!(
            terms.iter().any(|term| term.contains("Alice"))
                && terms.iter().any(|term| term.contains("Bob")),
            "author alternatives: {item:?}"
        );
        let lowered = without_line_indentation(&lowered_terminal(source));
        assert!(
            lowered.contains("Alice\nBob") && lowered.contains("BODY"),
            "author split vanished: {lowered:?}"
        );
    }
}

#[test]
fn nested_paragraph_authors_use_the_same_ordered_execution_stream() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh DESCRIPTION\n.Dq An -split An Alice An Bob\n.No END\n",
    );
    let native = without_line_indentation(&native_terminal(source));
    assert!(native.contains("“\nAlice\nBob” END"), "native: {native:?}");
    let lowered = without_line_indentation(&lowered_terminal(source));
    assert!(
        lowered.contains("“\nAlice\nBob” END"),
        "nested author execution: {lowered:?}"
    );
}

#[test]
fn block_authors_execute_each_mode_transition_once() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh AUTHORS\n.No PREFIX\n.An Alice\n.An Bob\n",
    );
    let native = without_line_indentation(&native_terminal(source));
    assert!(native.contains("PREFIX Alice\nBob"), "native: {native:?}");
    let lowered = without_line_indentation(&lowered_terminal(source));
    assert!(
        lowered.contains("PREFIX Alice\nBob"),
        "author mode executed more than once: {lowered:?}"
    );
}

#[test]
fn detached_definition_heads_project_author_flushes_by_list_style() {
    let cases = [
        ("inset", "Alice\nBob BODY"),
        ("tag", "Alice  Bob\n"),
        ("hang", "Alice Bob BODY"),
        ("ohang", "Alice\nBob\nBODY"),
    ];
    for (style, expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It Xo An -split An Alice An Bob Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source)).replace('\u{a0}', " ");
        assert!(native.contains(expected), "native {style}: {native:?}");
        let lowered = without_line_indentation(&lowered_terminal(&source)).replace('\u{a0}', " ");
        assert!(lowered.contains(expected), "lowered {style}: {lowered:?}");
    }
}

#[test]
fn control_only_authors_preserve_native_rows_and_field_origins() {
    for control in [r"\&", r"\p"] {
        for style in ["inset", "tag", "hang", "ohang"] {
            let source = format!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It Xo\n.An -split\n.An {control}\n.An Bob\n.Xc\n.No BODY\n.El\n"
            );
            let native = native_terminal(&source).replace('\u{a0}', " ");
            let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
            match style {
                "inset" => {
                    assert!(
                        native.contains("DESCRIPTION\n\n     Bob BODY"),
                        "native: {native:?}"
                    );
                    assert!(
                        lowered.contains("DESCRIPTION\n\nBob BODY"),
                        "lowered: {lowered:?}"
                    );
                }
                "ohang" => {
                    assert!(
                        native.contains("DESCRIPTION\n\n     Bob\n     BODY"),
                        "native: {native:?}"
                    );
                    assert!(
                        lowered.contains("DESCRIPTION\n\nBob\nBODY"),
                        "lowered: {lowered:?}"
                    );
                }
                "tag" | "hang" => {
                    let native_line = native.lines().find(|line| line.contains("Bob")).unwrap();
                    let lowered_line = lowered.lines().find(|line| line.contains("Bob")).unwrap();
                    assert_eq!(
                        lowered_line,
                        native_line.trim_start(),
                        "{style} {control} must retain the native term origin and body column"
                    );
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn no_break_flush_releases_a_word_boundary_after_fixed_run_in_cells() {
    for (style, spaces) in [("inset", 2), ("diag", 3)] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It A\n.mc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let expected = format!("A{}BODY", " ".repeat(spaces));
        let normalized_native = native.replace('\u{a0}', " ");
        assert!(
            normalized_native.contains(&expected),
            "native {style} gap: {native:?}"
        );

        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower run-in margin");
        let item = first_definition_item(query.document.as_ref().unwrap());
        let description = item
            .description
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => Some(super::inline_text(children)),
                _ => None,
            })
            .expect("run-in description");
        assert_eq!(description, format!("{}BODY", " ".repeat(spaces)));
    }
}

#[test]
fn no_break_field_separator_survives_spacing_modes() {
    for (style, spaces) in [("inset", 2), ("diag", 3)] {
        for spacing in ["", ".Sm off\n"] {
            let source = format!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It A\n{spacing}.mc\n.No BODY\n.El\n"
            );
            let native = native_terminal(&source).replace('\u{a0}', " ");
            let expected = format!("A{}BODY", " ".repeat(spaces));
            assert!(native.contains(&expected), "native {style}: {native:?}");
            let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
            assert!(
                lowered.contains(&expected),
                "lowered {style} {spacing:?}: {lowered:?}"
            );
        }
    }
}

#[test]
fn no_break_flush_trims_word_padding_but_keeps_its_field_separator() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh DESCRIPTION\n.No A \"\"\n.mc\n.No BODY\n",
    );
    let native = native_terminal(source);
    assert!(native.contains("A BODY"), "native: {native:?}");
    assert!(!native.contains("A  BODY"), "native: {native:?}");

    let lowered = lowered_terminal(source);
    assert!(lowered.contains("A BODY"), "lowered: {lowered:?}");
    assert!(!lowered.contains("A  BODY"), "lowered: {lowered:?}");
}

#[test]
fn no_break_field_separator_survives_transparent_and_tight_nodes() {
    let cases = [
        (".Tg mark\n.No BODY", "A BODY"),
        (".Ns\n.No BODY", "A BODY"),
        (".No )", "A )"),
    ];
    for (tail, expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No A\n.mc\n{tail}\n"
        );
        let native = native_terminal(&source);
        assert!(native.contains(expected), "native {tail:?}: {native:?}");
        let lowered = lowered_terminal(&source);
        assert!(lowered.contains(expected), "lowered {tail:?}: {lowered:?}");
    }
}

#[test]
fn no_break_flush_keeps_nonbreaking_formatter_cells_distinct_from_padding() {
    for escape in [r"\~", r"\0"] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No {escape}\n.mc\n.No BODY\n"
        );
        let native = native_terminal(&source).replace('\u{a0}', " ");
        assert!(native.contains("  BODY"), "native {escape}: {native:?}");
        let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
        assert!(lowered.contains("  BODY"), "lowered {escape}: {lowered:?}");
    }
}

#[test]
fn no_break_field_preserves_formatter_word_order_for_empty_fixed_and_zero_width_words() {
    let cases = [
        ("empty", ".No \"\"", "A  BODY"),
        ("empty-spacing-off", ".Sm off\n.No \"\"", "A BODY"),
        ("empty-tight", ".Ns\n.No \"\"", "A  BODY"),
        ("nonbreaking-space", r".No \~", "A   BODY"),
        ("fixed-width-space", r".No \0", "A   BODY"),
        ("zero-advance", r".No \zX", "A XBODY"),
    ];
    for (label, middle, expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No A\n.mc\n{middle}\n.No BODY\n"
        );
        let native = native_terminal(&source).replace('\u{a0}', " ");
        assert!(native.contains(expected), "native {label}: {native:?}");
        let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
        assert!(lowered.contains(expected), "lowered {label}: {lowered:?}");
    }
}

#[test]
fn invisible_formatter_fields_still_own_their_empty_word_boundary() {
    for (label, first) in [
        ("zero-width", r"\&"),
        ("word-end-break", r"\p"),
        ("zero-advance-break", r"\z\p"),
    ] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No {first}\n.mc\n.No \"\"\n.No BODY\n"
        );
        let native = native_terminal(&source);
        assert!(
            native.contains("\n       BODY"),
            "native {label}: {native:?}"
        );

        let query = mant_loader::load_roff_bytes(source.as_bytes())
            .expect("lower invisible no-break field");
        let document = query.document.as_ref().expect("lowered document");
        let paragraph = document.sections[1]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => Some(super::inline_text(children)),
                _ => None,
            })
            .expect("description paragraph");
        assert_eq!(paragraph, "  BODY", "lowered {label}");
    }
}

#[test]
fn control_only_word_end_break_drops_a_trailing_no_break_field_separator() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh DESCRIPTION\n.No A\n.mc\n.No \\p\n.No BODY\n",
    );

    // Pinned CVS `term_field()` correctly drops the separator because the
    // control-only field has no printable cell.  Its current renderer then
    // also loses BODY at this edge; ManT deliberately follows groff's
    // content-preserving result while retaining CVS's no-trailing-blank
    // field contract.
    let native = native_terminal(source);
    assert!(native.contains("\n     A\n"), "native: {native:?}");
    assert!(!native.contains("\n     A \n"), "native: {native:?}");

    let lowered = lowered_terminal(source);
    assert!(lowered.contains("\nA\nBODY"), "lowered: {lowered:?}");
    assert!(!lowered.contains("\nA \n"), "lowered: {lowered:?}");
}

#[test]
fn run_in_no_break_flush_preserves_fixed_and_pending_cells() {
    for (style, head, spaces) in [
        ("inset", "", 0),
        ("diag", "", 3),
        ("inset", r"A\z", 2),
        ("diag", r"A\z", 2),
    ] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It {head}\n.mc\n.No BODY\n.El\n"
        );
        let expected = format!("{}BODY", " ".repeat(spaces));
        let native = native_terminal(&source).replace('\u{a0}', " ");
        assert!(
            native.contains(&expected),
            "native {style} {head:?}: {native:?}"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower run-in cells");
        let item = first_definition_item(query.document.as_ref().unwrap());
        let description = item
            .description
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => Some(super::inline_text(children)),
                _ => None,
            })
            .expect("run-in description");
        assert_eq!(description, expected, "lowered {style} {head:?}: {item:?}");
    }
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

#[test]
#[allow(clippy::too_many_lines)] // This table is one pinned native field ledger.
fn definition_head_controls_settle_the_same_native_field() {
    // Verified against the pinned CVS `termp_it_pre/post()`,
    // `roff_term_pre_br/sp/ti()`, and `term_flushln()`.  NOBREAK, BRIND,
    // HANG, trailspace, and the body origin form one formatter field; none of
    // these requests may be lowered as an unrelated paragraph break.
    let cases = [
        (
            "tag br overrun",
            "tag",
            "6n",
            ".br",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n        A\nBob\n        BODY",
        ),
        (
            "tag br fit",
            "tag",
            "12n",
            ".br",
            "LONGTEXT      A\nBob\nBODY",
            "LONGTEXT      A\nBob\n              BODY",
        ),
        (
            "tag temporary indent",
            "tag",
            "6n",
            ".ti 2n",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n  A\nBob\n        BODY",
        ),
        (
            "tag temporary indent fit",
            "tag",
            "12n",
            ".ti 2n",
            "LONGTEXT  A\nBob\nBODY",
            "LONGTEXT  A\nBob\n              BODY",
        ),
        (
            "tag vertical space",
            "tag",
            "6n",
            ".sp 1",
            "LONGTEXT\n\nA\nBob\nBODY",
            "LONGTEXT\n\n        A\nBob\n        BODY",
        ),
        (
            "tag fill boundary",
            "tag",
            "6n",
            ".nf",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n        A\nBob\n        BODY",
        ),
        (
            "tag fill boundary fit",
            "tag",
            "12n",
            ".nf",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag vertical space fit",
            "tag",
            "12n",
            ".sp 1",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "hang br overrun",
            "hang",
            "6n",
            ".br",
            "LONGTEXT ABobBODY",
            "LONGTEXT ABobBODY",
        ),
        (
            "hang temporary indent overrun",
            "hang",
            "6n",
            ".ti 2n",
            "LONGTEXT ABobBODY",
            "LONGTEXT ABobBODY",
        ),
        (
            "hang vertical space closes its occupied row",
            "hang",
            "6n",
            ".sp 1",
            "LONGTEXT\nABobBODY",
            "LONGTEXT\n        ABobBODY",
        ),
        (
            "hang br fit",
            "hang",
            "12n",
            ".br",
            "LONGTEXT      ABobBODY",
            "LONGTEXT      ABobBODY",
        ),
        (
            "hang temporary indent",
            "hang",
            "12n",
            ".ti 2n",
            "LONGTEXT ABob BODY",
            "LONGTEXT ABob BODY",
        ),
        (
            "hang vertical space fit",
            "hang",
            "12n",
            ".sp 1",
            "LONGTEXT\nABobBODY",
            "LONGTEXT\n              ABobBODY",
        ),
        (
            "hang fill boundary overrun",
            "hang",
            "6n",
            ".nf",
            "LONGTEXTABob\nBODY",
            "LONGTEXTABob\n        BODY",
        ),
        (
            "hang fill boundary fit",
            "hang",
            "12n",
            ".nf",
            "LONGTEXT      ABob\nBODY",
            "LONGTEXT      ABob\n              BODY",
        ),
    ];

    for (label, style, width, request, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width {width}\n.It Xo\n.No LONGTEXT\n{request}\n.No A\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(
            native.contains(native_expected),
            "{label} native output: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "{label} lowered output: {lowered:?}"
        );
    }
}

#[test]
fn no_break_field_retains_brind_and_hang_for_following_controls() {
    // Verified against CVS `roff_term_pre_mc()`: only NOBREAK and NOSPACE
    // are cleared after the flush. BRIND/HANG and the field geometry remain
    // live for a following request, even though `.mc` itself emitted no text.
    let cases = [
        (
            "tag/br",
            "tag",
            ".br",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag/ti",
            "tag",
            ".ti 4n",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n    A\nBob\n              BODY",
        ),
        (
            "tag/sp",
            "tag",
            ".sp 1",
            "LONGTEXT\n\nA\nBob\nBODY",
            "LONGTEXT\n\n              A\nBob\n              BODY",
        ),
        (
            "tag/nf",
            "tag",
            ".nf",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "hang/br",
            "hang",
            ".br",
            "LONGTEXT      ABobBODY",
            "LONGTEXT      ABobBODY",
        ),
        (
            "hang/ti",
            "hang",
            ".ti 4n",
            "LONGTEXT ABob BODY",
            "LONGTEXT ABob BODY",
        ),
        (
            "hang/sp",
            "hang",
            ".sp 1",
            "LONGTEXT\nABobBODY",
            "LONGTEXT\n              ABobBODY",
        ),
        (
            "hang/nf",
            "hang",
            ".nf",
            "LONGTEXT      ABob\nBODY",
            "LONGTEXT      ABob\n              BODY",
        ),
    ];

    for (label, style, request, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width 12n\n.It Xo\n.No LONGTEXT\n.mc\n{request}\n.No A\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(
            native.contains(native_expected),
            "{label} native output: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "{label} lowered output: {lowered:?}"
        );
    }
}

#[test]
fn no_break_field_classifies_following_words_before_a_control() {
    // Verified against CVS `term_word()`, `term_fill()`, and
    // `roff_term_pre_br()`. Empty/NBRZW words do not advance the visual
    // field, while a visible word does; BRIND and HANG remain independent of
    // both facts after `.mc` clears NOBREAK and NOSPACE.
    let cases = [
        (
            "tag/empty",
            "tag",
            r#""""#,
            ".No A\n",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag/ignore",
            "tag",
            r"\&",
            ".No A\n",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag/visible",
            "tag",
            "A",
            "",
            "LONGTEXT   A\nBob\nBODY",
            "LONGTEXT   A\nBob\n              BODY",
        ),
        (
            "hang/empty",
            "hang",
            r#""""#,
            ".No A\n",
            "LONGTEXT      ABobBODY",
            "LONGTEXT      ABobBODY",
        ),
        (
            "hang/ignore",
            "hang",
            r"\&",
            ".No A\n",
            "LONGTEXT      ABobBODY",
            "LONGTEXT      ABobBODY",
        ),
        (
            "hang/visible",
            "hang",
            "A",
            "",
            "LONGTEXT  ABobBODY",
            "LONGTEXT  ABobBODY",
        ),
        (
            "tag/empty before control-only author",
            "tag",
            r#""""#,
            "",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "tag/ignore before control-only author",
            "tag",
            r"\&",
            "",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "hang/empty before control-only author",
            "hang",
            r#""""#,
            "",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
        (
            "hang/ignore before control-only author",
            "hang",
            r"\&",
            "",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
    ];

    for (label, style, operand, after_control, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width 12n\n.It Xo\n.No LONGTEXT\n.mc\n.No {operand}\n.br\n{after_control}.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(
            native.contains(native_expected),
            "{label} native output: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "{label} lowered output: {lowered:?}"
        );
    }
}

#[test]
#[allow(clippy::too_many_lines)] // This table is one pinned native field ledger.
fn control_only_author_handoffs_settle_buffer_and_device_rows_separately() {
    // Verified first with the pinned CVS renderer.  An empty word has no
    // native field cell at a tight list-head boundary, `\&` does have one,
    // and a bare `\z` only arms BACKAFTER.  After `.mc`, `viscol` keeps the
    // device row occupied even when the current field buffer is empty, so a
    // later control must flush and clear BACKAFTER before Bob executes.
    let cases = [
        (
            "tag device row, empty before ti",
            "tag",
            ".No LONGTEXT\n.mc\n",
            r#""""#,
            ".ti 4n",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "tag device row, zero-width cell before ti",
            "tag",
            ".No LONGTEXT\n.mc\n",
            r"\&",
            ".ti 4n",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "tag device row, bare zero before ti",
            "tag",
            ".No LONGTEXT\n.mc\n",
            r"\z",
            ".ti 4n",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "hang device row, empty before ti",
            "hang",
            ".No LONGTEXT\n.mc\n",
            r#""""#,
            ".ti 4n",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
        (
            "hang device row, zero-width cell before ti",
            "hang",
            ".No LONGTEXT\n.mc\n",
            r"\&",
            ".ti 4n",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
        (
            "hang device row, bare zero before ti",
            "hang",
            ".No LONGTEXT\n.mc\n",
            r"\z",
            ".ti 4n",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
        (
            "hang device row, bare zero before fill boundary",
            "hang",
            ".No LONGTEXT\n.mc\n",
            r"\z",
            ".nf",
            "LONGTEXTBob\nBODY",
            "LONGTEXTBob\n              BODY",
        ),
        (
            "tag empty before ti",
            "tag",
            "",
            r#""""#,
            ".ti 4n",
            "Bob\nBODY",
            "Bob\n              BODY",
        ),
        (
            "tag zero-width cell before ti",
            "tag",
            "",
            r"\&",
            ".ti 4n",
            "Bob\nBODY",
            "Bob\n              BODY",
        ),
        (
            "tag bare zero before br",
            "tag",
            "",
            r"\z",
            ".br",
            "ob\nBODY",
            "ob\n              BODY",
        ),
        (
            "tag bare zero before ti",
            "tag",
            "",
            r"\z",
            ".ti 4n",
            "ob\nBODY",
            "ob\n              BODY",
        ),
        (
            "tag bare zero before vertical space",
            "tag",
            "",
            r"\z",
            ".sp 1",
            "\nob\nBODY",
            "\nob\n              BODY",
        ),
        (
            "tag bare zero before fill boundary",
            "tag",
            "",
            r"\z",
            ".nf",
            "ob\nBODY",
            "ob\n              BODY",
        ),
        (
            "hang zero-width cell before ti",
            "hang",
            "",
            r"\&",
            ".ti 4n",
            "Bob           BODY",
            "Bob           BODY",
        ),
        (
            "hang zero-width cell before fill boundary",
            "hang",
            "",
            r"\&",
            ".nf",
            "Bob\nBODY",
            "Bob\n              BODY",
        ),
        (
            "hang bare zero before br",
            "hang",
            "",
            r"\z",
            ".br",
            "ob            BODY",
            "ob            BODY",
        ),
        (
            "hang bare zero before ti",
            "hang",
            "",
            r"\z",
            ".ti 4n",
            "ob            BODY",
            "ob            BODY",
        ),
        (
            "hang bare zero before vertical space",
            "hang",
            "",
            r"\z",
            ".sp 1",
            "\nob            BODY",
            "\nob            BODY",
        ),
        (
            "hang bare zero before fill boundary",
            "hang",
            "",
            r"\z",
            ".nf",
            "ob\nBODY",
            "ob\n              BODY",
        ),
        (
            "tag empty before explicit break and visible word",
            "tag",
            "",
            r#""""#,
            ".br\n.No A",
            "A\nBob\nBODY",
            "              A\nBob\n              BODY",
        ),
        (
            "tag bare zero before explicit break and visible word",
            "tag",
            "",
            r"\z",
            ".br\n.No A",
            "A\nBob\nBODY",
            "              A\nBob\n              BODY",
        ),
        (
            "hang empty before explicit break and visible word",
            "hang",
            "",
            r#""""#,
            ".br\n.No A",
            "ABobBODY",
            "              ABobBODY",
        ),
        (
            "hang bare zero before explicit break and visible word",
            "hang",
            "",
            r"\z",
            ".br\n.No A",
            "ABobBODY",
            "              ABobBODY",
        ),
    ];

    for (label, style, prefix, operand, control, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width 12n\n.It Xo\n{prefix}.No {operand}\n{control}\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(
            native.contains(native_expected),
            "{label} native output: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "{label} lowered output: {lowered:?}"
        );
    }
}

#[test]
fn formatter_boundaries_precede_pending_zero_advance_glyphs() {
    // CVS `term_word()` buffers each word boundary before decoding `\zX`;
    // BACKBEFORE then lets X overwrite the following word's boundary.  The
    // ordering is the same for an explicit empty word, `\&`, and the fixed
    // blank glyph `\0`, with or without `.mc` settling the previous field.
    for predecessor in [r#""""#, r"\&", r"\0", r"\~"] {
        for zero_expression in [r"\zX", r"\p\zX", r"\zX\p"] {
            for margin_request in ["", ".mc\n"] {
                let source = format!(
                    ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No A\n.No {predecessor}\n{margin_request}.No {zero_expression}\n.No BODY\n"
                );
                let native = without_line_indentation(&native_terminal(&source));
                assert!(
                    native.contains("XBODY"),
                    "{predecessor:?}/{zero_expression:?}/{margin_request:?} native output: {native:?}"
                );
                let lowered = lowered_terminal(&source);
                assert!(
                    lowered.contains("XBODY"),
                    "{predecessor:?}/{zero_expression:?}/{margin_request:?} lowered output: {lowered:?}"
                );
                assert!(
                    !lowered.contains("X BODY"),
                    "boundary moved after pending glyph: {lowered:?}"
                );
            }
        }
    }
}
