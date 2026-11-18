//! Fixed-CVS cases for repeated word effects and source-owned audit findings.

use super::{Case, OrdinalDisposition};

use OrdinalDisposition::{Recovered, Retained, Uncertain};

type Template = (
    &'static str,
    &'static str,
    &'static str,
    OrdinalDisposition,
    &'static str,
);

// Exact inputs were checked against pristine ASCII/UTF-8/tree/HTML.
// Repeated BACKAFTER retains BACKBEFORE, En executes the native Es words,
// PILE bypasses a one-argument row wrapper, and ce/rj skip their count.
const TEMPLATES: [Template; 22] = [
    ("plain", "NUM.", "BODYNUM", Recovered, ""),
    ("held", r"\zXNUM.", "BODYNUM", Uncertain, ""),
    ("twice", r"\zX\zVNUM.", "BODYNUM", Uncertain, ""),
    ("thrice", r"\zX\zV\zYNUM.", "BODYNUM", Uncertain, ""),
    (
        "split",
        r"No \zX Ns No \zV Ns No NUM .",
        "BODYNUM",
        Uncertain,
        "",
    ),
    (
        "styled",
        r"Sy \zX Ns Em \zV Ns No NUM .",
        "BODYNUM",
        Uncertain,
        "",
    ),
    (
        "period",
        "En NUM",
        "BODYNUM",
        Recovered,
        ".Es \\& \\N'46'\n",
    ),
    (
        "styled-period",
        "En Sy NUM",
        "BODYNUM",
        Recovered,
        ".Es \\& \\N'46'\n",
    ),
    (
        "scope-period",
        "Xo\n.En NUM\n.Xc",
        "BODYNUM",
        Recovered,
        ".Es \\& \\N'46'\n",
    ),
    ("prefix", "Pf NUM .", "BODYNUM", Recovered, ""),
    ("atom", "NUM.", ".EQ\n\"\"\n.EN", Uncertain, ""),
    ("pile", "NUM.", ".EQ\npile { \"\" }\n.EN", Uncertain, ""),
    (
        "pile-rows",
        "NUM.",
        ".EQ\npile { \"\" above \"\" }\n.EN",
        Uncertain,
        "",
    ),
    (
        "left-pile",
        "NUM.",
        ".EQ\nlpile { \"\" }\n.EN",
        Uncertain,
        "",
    ),
    (
        "pile-zero",
        "NUM.",
        ".EQ\npile { \"\\&\" }\n.EN",
        Uncertain,
        "",
    ),
    ("empty-pile", "NUM.", ".EQ\npile { }\n.EN", Recovered, ""),
    ("center-count", "NUM.", ".ce 0", Retained, ""),
    ("right-count", "NUM.", ".rj 0", Retained, ""),
    ("center-content", "NUM.", ".ce 1\nBODYNUM", Recovered, ""),
    ("right-content", "NUM.", ".rj 1\nBODYNUM", Recovered, ""),
    ("config", "NUM.", ".EQ\ngsize 12\n.EN", Retained, ""),
    (
        "matrix-zero",
        "NUM.",
        ".EQ\nmatrix { lcol { \"\" } }\n.EN",
        Uncertain,
        "",
    ),
];

pub(crate) fn cases() -> impl Iterator<Item = Case> {
    TEMPLATES
        .into_iter()
        .flat_map(|(name, head, body, disposition, before)| {
            [1_u8, 2, 3].map(move |mask| {
                let mut source = header().to_owned();
                source.push_str(before);
                source.push_str(".Bl -tag -width 6n\n");
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
                    name: format!("source-{name}-{mask}"),
                    source,
                    source_line,
                    disposition,
                }
            })
        })
}

pub(crate) fn nested() -> impl Iterator<Item = Case> {
    ["top", "body", "head"].into_iter().flat_map(|owner| {
        [false, true].map(move |uncertain| {
            let (intro, post) = match owner {
                "body" => (".Bl -tag -width Ds\n.It OUTER\n", ".El\n"),
                "head" => (".Bl -tag -width Ds\n.It Xo\n", ".Xc\nOUTER BODY\n.El\n"),
                _ => ("", ""),
            };
            let mut source = format!("{}{intro}.Bl -tag -width 6n\n", header());
            let source_line = u32::try_from(source.lines().count()).unwrap();
            let prefix = if uncertain { r"\zX" } else { "" };
            for number in 1..=2 {
                source.push_str(&format!(".It {prefix}{number}.\nBODY{number}\n"));
            }
            source.push_str(&format!(".El\n{post}.Sh END PROBE\nDone.\n"));
            Case {
                name: format!("{owner}-{uncertain}"),
                source,
                source_line,
                disposition: if uncertain {
                    OrdinalDisposition::Uncertain
                } else {
                    OrdinalDisposition::Recovered
                },
            }
        })
    })
}

fn header() -> &'static str {
    concat!(
        ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n",
        ".Nd test page\n.Sh DESCRIPTION\n"
    )
}
