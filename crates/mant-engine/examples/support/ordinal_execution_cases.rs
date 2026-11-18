//! Independent fixed-CVS observations across node and generated-word boundaries.

use super::super::OrdinalDisposition;

#[path = "ordinal_source_cases.rs"]
pub(crate) mod source;

pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) source_line: u32,
    pub(crate) disposition: OrdinalDisposition,
}

pub(crate) fn cases() -> impl Iterator<Item = Case> {
    use OrdinalDisposition::{Recovered, Retained, Uncertain};
    // term.c::encode1() carries BACKBEFORE/BACKAFTER through TEXT and font
    // nodes. Fn/Fo and the normalized Es/En write their own delimiters.
    // eqn_term.c::eqn_box() visits no word for the empty equation root.
    [
        ("plain", "NUM.", "BODYNUM", Recovered, ""),
        ("same-word", r"\zXNUM.", "BODYNUM", Uncertain, ""),
        ("prefix", "Pf NUM .", "BODYNUM", Recovered, ""),
        ("flag", "NUM.", ".Fl", Recovered, ""),
        ("size", "NUM.", r".No \s12", Retained, ""),
        ("color", "NUM.", r".No \m[red]", Retained, ""),
        ("split", r"No \zX Ns No NUM .", "BODYNUM", Uncertain, ""),
        ("style", r"Sy \zX Ns Em NUM .", "BODYNUM", Uncertain, ""),
        ("armed", r"No \z Ns No XNUM .", "BODYNUM", Uncertain, ""),
        (
            "line",
            "Xo\n.No \\zX\\c\n.No NUM .\n.Xc",
            "BODYNUM",
            Uncertain,
            "",
        ),
        ("function", "NUM.", r".Fn \&", Recovered, ""),
        ("function-scope", "NUM.", ".Fo \\&\n.Fc", Recovered, ""),
        ("custom-enclosure", "NUM.", ".En", Recovered, ".Es < >\n"),
        ("empty-equation", "NUM.", ".EQ\n.EN", Retained, ""),
        ("equation", "NUM.", ".EQ\nx\n.EN", Recovered, ""),
    ]
    .into_iter()
    .flat_map(|(name, head, body, disposition, pre)| {
        [("first", 1_u8), ("last", 2), ("both", 3)].map(|(mask_name, mask)| {
            let mut source = String::from(concat!(
                ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n",
                ".Nd probe page\n.Sh DESCRIPTION\n"
            ));
            source.push_str(pre);
            source.push_str(".Bl -tag -width 4n\n");
            let source_line = u32::try_from(source.lines().count()).unwrap();
            for (bit, number) in [(1, "1"), (2, "2")] {
                let (head, body) = if mask & bit != 0 {
                    (head, body)
                } else {
                    ("NUM.", "BODYNUM")
                };
                source.push_str(&format!(
                    ".It {}\n{}\n",
                    head.replace("NUM", number),
                    body.replace("NUM", number)
                ));
            }
            source.push_str(".El\n.Sh END PROBE\nDone.\n");
            Case {
                name: format!("{name}-{mask_name}"),
                source,
                source_line,
                disposition,
            }
        })
    })
}

pub(crate) fn equation_edges() -> impl Iterator<Item = Case> {
    use OrdinalDisposition::{Recovered, Retained, Uncertain};
    // Fixed eqn_term.c emits parentheses for the empty pile/list scopes.
    // The empty matrix and empty atoms are authored mathematical objects
    // without a provable glyph; gsize leaves only the configuration root.
    [
        ("pile", "pile { }", Recovered),
        ("group", "{ }", Recovered),
        ("matrix", "matrix { }", Uncertain),
        ("atom", "\"\"", Uncertain),
        ("zero-width", "\"\\&\"", Uncertain),
        ("configuration", "gsize 12", Retained),
    ]
    .into_iter()
    .flat_map(|(name, expression, disposition)| {
        [1_u8, 2, 3].map(move |mask| {
            let mut source = concat!(
                ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n",
                ".Nd test\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n"
            )
            .to_owned();
            for (bit, number) in [(1, 1), (2, 2)] {
                source.push_str(&format!(".It {number}.\n"));
                if mask & bit == 0 {
                    source.push_str(&format!("BODY{number}\n"));
                } else {
                    source.push_str(&format!(".EQ\n{expression}\n.EN\n"));
                }
            }
            source.push_str(".El\n");
            Case {
                name: format!("equation-{name}-{mask}"),
                source,
                source_line: 8,
                disposition,
            }
        })
    })
}
