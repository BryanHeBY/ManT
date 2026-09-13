use super::*;

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
