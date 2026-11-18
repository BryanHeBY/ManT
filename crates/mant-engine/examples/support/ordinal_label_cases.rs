//! Fixed CVS shows the same dt/dd owners, with the authored delimiters and
//! style boundaries below. The product's documented recovery policy supplies
//! the independent yes/no disposition.

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
    })
}
