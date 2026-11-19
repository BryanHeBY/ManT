//! Fixed CVS shows the same dt/dd owners, with the authored delimiters and
//! style boundaries below. The product's documented recovery policy supplies
//! the independent yes/no disposition.

#[path = "ordinal_execution_cases.rs"]
pub(crate) mod execution;
#[path = "ordinal_payload_cases.rs"]
pub(crate) mod payloads;

const HEADER: &str = concat!(
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
    ".Nm probe\n.Nd exercise descriptors\n.Sh DESCRIPTION\n",
);

// Fixed tree assigns all three Bl owners 26:2. HTML has a bullet list
// followed by two separate definition owners and the matching descriptions.
pub(crate) const SAME_ORIGIN: &str = concat!(
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
    ".Nm probe\n.Nd exercise descriptors\n.Sh DESCRIPTION\n",
    ".de LISTS\n.Bl -bullet\n.It\nBULLET\n.El\n",
    ".Bl -tag -width Ds\n.It 1.\nALPHA\n.It 2.\nBETA\n.El\n",
    ".Bl -tag -width Ds\n.It [1]\nGAMMA\n.It [2]\nDELTA\n.El\n..\n.LISTS\n"
);

const REVIEW_HEADER: &str = concat!(
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
    ".Nm probe\n.Nd probe page\n.Sh DESCRIPTION\n",
);

// Fixed -Thtml exposes three independent hrefs and matching identifiers.
pub(crate) const LINK_JSON: &str = concat!(
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
    ".Nm probe\n.Nd probe page\n.Sh DESCRIPTION\n",
    ".Bl -tag -width Ds\n.It Sx 1 .\nBODY1\n.It Sx 2 .\nBODY2\n.It Sx 3 .\nBODY3\n.El\n",
    ".Sh 1\nDEST1\n.Sh 2\nDEST2\n.Sh 3\nDEST3\n",
);

pub(crate) fn execution_inputs() -> impl Iterator<Item = (String, [&'static str; 3], bool)> {
    [
        ("", [r"1\N'46'", r"2\N'46'", r"3\N'46'"], ["1.", "2.", "3."], true),
        ("", [r"1\Nx46x", r"2\Nx46x", r"3\Nx46x"], ["1.", "2.", "3."], true),
        (".Sm off\n", ["Sy 1 No 0 .", "Sy 2 No 0 .", "Sy 3 No 0 ."], ["10.", "20.", "30."], false),
        (".Sm on\n", ["Sy 1 No 0 .", "Sy 2 No 0 .", "Sy 3 No 0 ."], ["1 0.", "2 0.", "3 0."], false),
        (".Sm\n", ["Sy 1 No 0 .", "Sy 2 No 0 .", "Sy 3 No 0 ."], ["10.", "20.", "30."], false),
        ("", [r"\~1.", r"\~2.", r"\~3."], ["\u{a0}1.", "\u{a0}2.", "\u{a0}3."], false),
        ("", [r"1.\0", r"2.\0", r"3.\0"], ["1.\u{a0}", "2.\u{a0}", "3.\u{a0}"], false),
        ("", [r"\N'160'1.", r"\N'160'2.", r"\N'160'3."], ["\u{a0}1.", "\u{a0}2.", "\u{a0}3."], false),
    ].into_iter().map(|(before, [first, second, third], labels, recover)| {
        let source = format!(
            "{REVIEW_HEADER}{before}.Bl -tag -width Ds\n.It {first}\nBODY1\n.It {second}\nBODY2\n.It {third}\nBODY3\n.El\n"
        );
        (source, labels, recover)
    }).chain([".Sm on", ".TS\nl.\nTABLE\n.TE"].into_iter().map(|control| (format!(
        "{REVIEW_HEADER}.Sm off\n.Bl -tag -width Ds\n.It Sy 1 No 0 .\nBODY1\n.It Sy 2 No 0 .\nBODY2\n{control}\n.It Sy 3 No 0 .\nBODY3\n.El\n"
    ), ["10.", "20.", "3 0."], false)))
}

pub(crate) fn body_and_boundary_inputs() -> impl Iterator<Item = (String, [String; 2], bool)> {
    // A glyphless word can own a row in no-fill, while a pure continuation
    // never writes a cell. CVS quote pre/post and Ap emit their own glyphs.
    let bodies = [
        (".Ap", true),
        (".Pq", true),
        (".Po\n.Pc", true),
        (r".No \zX", true),
        (r".No \~", true),
        (r".No \c", false),
        (r".No \p", false),
        (r".No \&", false),
        (r".No \z", false),
        (r".No \fB", false),
        (".sp 0", true),
        (".sp 1", true),
        (".sp -1\n.sp 1", true),
        (".nf\n\n.fi", true),
        (".nf\n.No \\&\n.fi", true),
        (".nf\n.No \\p\n.fi", true),
        (".nf\n.No \\c\n.fi", false),
        (".nf\n.No \\c\n\n.fi", false),
    ]
    .into_iter()
    .map(|(body, recover)| {
        (
            format!("{REVIEW_HEADER}.Bl -tag -width Ds\n.It 1.\n{body}\n.It 2.\n{body}\n.El\n"),
            ["1.".to_owned(), "2.".to_owned()],
            recover,
        )
    });
    let labels = [
        ("Pq NUM", "(NUM)", true),
        ("Po NUM Pc", "(NUM)", true),
        (r"NUM.\c", "NUM.", true),
        (r"NUM.\p", "NUM.", true),
        (r"No NUM\c .", "NUM.", true),
        (r"Pq NUM\p", "(NUM)", true),
        ("\"        NUM.\"", "        NUM.", false),
        ("\"NUM.        \"", "NUM.        ", false),
    ]
    .into_iter()
    .map(|(source_form, term, recover)| {
        let [first, second] = ["1", "2"].map(|number| source_form.replace("NUM", number));
        let labels = ["1", "2"].map(|number| term.replace("NUM", number));
        (
            format!(
                "{REVIEW_HEADER}.Bl -tag -width Ds\n.It {first}\nBODY1\n.It {second}\nBODY2\n.El\n"
            ),
            labels,
            recover,
        )
    });
    bodies.chain(labels)
}

pub(crate) fn body_needs_review() -> String {
    // The fixed terminal emits x for this overstrike; the bounded ordinal
    // observer records uncertainty rather than claiming a space or glyph.
    format!("{REVIEW_HEADER}.Bl -tag -width Ds\n.It 1.\n.No \\o'x'\n.It 2.\n.No \\o'x'\n.El\n")
}

pub(crate) fn bare_zero_advance_with_empty_row() -> String {
    // CVS leaves backspacing armed across an empty no-fill input boundary.
    // The second label in this input can no longer be certified by spelling.
    format!(
        "{REVIEW_HEADER}.Bl -tag -width Ds\n.It 1.\n.nf\n.No \\z\n\n.fi\n.It 2.\n.nf\n.No \\z\n\n.fi\n.El\n"
    )
}

fn evidence_page(head: &str, body: &str) -> String {
    let item = |number: &str| {
        let head = head.replace("NUM", number);
        let body = body.replace("NUM", number);
        format!(".It {head}\n{body}\n")
    };
    format!(
        ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n{}{}.El\n",
        item("1"),
        item("2")
    )
}

pub(crate) fn generated_and_control_inputs() -> impl Iterator<Item = (String, [String; 2], bool)> {
    // CVS termp_fl_pre emits '-' without children. Size and color operands
    // belong to their escapes; only a suffix can supply a visible BODY.
    [
        (".Fl", true),
        (".Pq", true),
        (r".No \c", false),
        (r".No \s12", false),
        (r".No \s(12", false),
        (r".No \s[12]", false),
        (r".No \s'12'", false),
        (r".No \m[red]", false),
        (r".No \m(re", false),
        (r".No \mR", false),
        (r".No \F[SYM]", false),
        (".Db GHOSTTOKEN", false),
        (r".No \s12VISIBLE", true),
        (r".No \m[red]VISIBLE", true),
    ]
    .into_iter()
    .map(|(body, recovered)| (evidence_page("NUM.", body), ordinal_terms(), recovered))
    .chain(
        ["Pf NUM .", r"\s12NUM.", r"\m[red]NUM.", r"NUM.\s[12]"]
            .into_iter()
            .map(|head| (evidence_page(head, "BODYNUM"), ordinal_terms(), true)),
    )
}

pub(crate) fn heads_needing_review() -> impl Iterator<Item = String> {
    // These CVS labels read as ordinary 1. and 2., but proving that result is
    // a layout decision (motion, overwritten cells, or overstrikes).
    [
        r"\h'0n'NUM.",
        r"\h'+0n'NUM.",
        r"\zXNUM.",
        r"\z\m[red]XNUM.",
        r"\o'XNUM'.",
    ]
    .into_iter()
    .map(|head| evidence_page(head, "BODYNUM"))
}

fn ordinal_terms() -> [String; 2] {
    ["1.".to_owned(), "2.".to_owned()]
}

pub(crate) fn inputs() -> impl Iterator<Item = (String, bool)> {
    [
        ("tag -width Ds", ["1.", "2.", "3."], true),
        ("tag -width Ds", [r"1\&.", r"2\&.", r"3\&."], true),
        ("tag -width Ds", [r"1\:.", r"2\:.", r"3\:."], true),
        ("tag -width Ds", [r"1\z.", r"2\z.", r"3\z."], true),
        ("tag -width Ds", [r"\[u0031].", r"\[u0032].", r"\[u0033]."], true),
        ("tag -width Ds", [r"1\fR)", r"2\fR)", r"3\fR)"], true),
        ("tag -width Ds", ["Sy 1 .", "Sy 2 .", "Sy 3 ."], true),
        ("tag -width Ds", ["Pq 1", "Pq 2", "Pq 3"], true),
        ("tag -width Ds", ["[1]", "[2]", "[3]"], false),
        ("tag -width Ds", ["001.", "002.", "003."], false),
        ("tag -width Ds", ["+1.", "+2.", "+3."], false),
        ("tag -width Ds", ["Sy 1 Li 1.", "Sy 2 Li 2.", "Sy 3 Li 3."], false),
        ("tag -width Ds", ["(1)", "Xo\n.EQ\n(2)\n.EN\n.Xc", "(3)"], false),
        ("tag -width Ds", ["1.", "Sx 2 .", "3."], false),
        ("hang -width Ds", ["1.", "2.", "3."], false),
        ("diag", ["1.", "2.", "3."], false),
        ("inset", ["1.", "2.", "3."], false),
        ("ohang", ["1.", "2.", "3."], false),
    ].into_iter().map(|(style, [first, second, third], recovered)| {
        let source = format!(
            "{HEADER}.Bl -{style}\n.It {first}\nBODY-1\n.It {second}\nBODY-2\n.It {third}\nBODY-3\n.El\n.Sh 1\nDEST-1\n.Sh 2\nDEST-2\n.Sh 3\nDEST-3\n"
        );
        (source, recovered)
    }).chain(execution_inputs().map(|(source, _, recovered)| (source, recovered)))
        .chain(body_and_boundary_inputs().map(|(source, _, recovered)| (source, recovered)))
        .chain(generated_and_control_inputs().map(|(source, _, recovered)| (source, recovered)))
        .chain(std::iter::once((LINK_JSON.to_owned(), false)))
}
