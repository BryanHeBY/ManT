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
    source_matrix(&TEMPLATES, "BODYNUM")
}

const GENERATED: [Template; 22] = [
    ("plain", "NUM.", "BODY", Recovered, ""),
    ("blank", "NUM.", r".No \&", Retained, ""),
    ("include", "NUM.", ".In header.h", Recovered, ""),
    ("include-zero", "NUM.", r".In \&", Recovered, ""),
    ("link-zero", "NUM.", r".Lk \& \&", Recovered, ""),
    ("reference", "NUM.", ".Rs\n.%A \\&\n.Re", Recovered, ""),
    ("zero-word", r"NUM\z\&.", "BODY", Recovered, ""),
    ("zero-motion", r"\z\h'0n'NUM.", "BODY", Recovered, ""),
    ("positive-motion", r"\z\h'2n'NUM.", "BODY", Recovered, ""),
    ("negative-motion", r"\z\h'-2n'NUM.", "BODY", Recovered, ""),
    ("absolute-motion", r"\z\h'|2n'NUM.", "BODY", Recovered, ""),
    (
        "split-motion",
        r"No \z Ns No \h'2n'NUM .",
        "BODY",
        Recovered,
        "",
    ),
    ("empty-atom", "NUM.", ".EQ\n\"\"\n.EN", Uncertain, ""),
    (
        "solo-pile",
        "NUM.",
        ".EQ\npile { \"\" }\n.EN",
        Uncertain,
        "",
    ),
    (
        "suffix-pile",
        "NUM.",
        ".EQ\n\"\" pile { \"\" }\n.EN",
        Recovered,
        "",
    ),
    (
        "prefix-pile",
        "NUM.",
        ".EQ\npile { \"\" } \"\"\n.EN",
        Recovered,
        "",
    ),
    (
        "sandwiched-pile",
        "NUM.",
        ".EQ\n\"\" pile { \"\" } \"\"\n.EN",
        Recovered,
        "",
    ),
    (
        "columns",
        "NUM.",
        ".EQ\nmatrix { lcol { \"\" } rcol { \"\" } }\n.EN",
        Recovered,
        "",
    ),
    (
        "bullet",
        "NUM.",
        ".Bl -bullet\n.It\n\\&\n.El",
        Recovered,
        "",
    ),
    ("enum", "NUM.", ".Bl -enum\n.It\n\\&\n.El", Recovered, ""),
    ("dash", "NUM.", ".Bl -dash\n.It\n\\&\n.El", Recovered, ""),
    (
        "hyphen",
        "NUM.",
        ".Bl -hyphen\n.It\n\\&\n.El",
        Recovered,
        "",
    ),
];

pub(crate) fn generated() -> impl Iterator<Item = Case> {
    source_matrix(&GENERATED, "BODY")
}

pub(crate) fn buffered_rows_and_drawn_labels() -> impl Iterator<Item = Case> {
    // Fixed CVS termp_d1_pre/post retain the row written by D1/Dl's \&.
    // Flattening visible strings would also miss nested list/table owners.
    // In term_word()/encode1(), the drawable stroke consumes BACKAFTER;
    // the following digits still expose their source ordinal candidate.
    // These draw-dependent placements stay in the source review tier.
    const CASES: [Template; 17] = [
        ("display-row", "NUM.", r".D1 \&", Recovered, ""),
        ("literal-row", "NUM.", r".Dl \&", Recovered, ""),
        (
            "plain-item",
            "NUM.",
            ".Bl -item\n.It\n\\&\n.El",
            Recovered,
            "",
        ),
        (
            "compact-item",
            "NUM.",
            ".Bl -item -compact\n.It\n\\&\n.El",
            Recovered,
            "",
        ),
        (
            "column",
            "NUM.",
            ".Bl -column \"x\"\n.It \\&\n.El",
            Recovered,
            "",
        ),
        ("rule-default", r"\z\l'1n'NUM.", "BODY", Uncertain, ""),
        ("rule-ascii", r"\z\l'1nX'NUM.", "BODY", Uncertain, ""),
        ("rule-blank", r"\z\l'1n 'NUM.", "BODY", Uncertain, ""),
        ("rule-to-margin", r"\z\l'0nX'NUM.", "BODY", Uncertain, ""),
        (
            "rule-negative-margin",
            r"\z\l'-1nX'NUM.",
            "BODY",
            Uncertain,
            "",
        ),
        ("rule-blank-margin", r"\z\l'0n 'NUM.", "BODY", Uncertain, ""),
        ("rule-unicode", r"\z\l'1n\[bu]'NUM.", "BODY", Uncertain, ""),
        ("rule-decimal", r"\z\l'1n\N'88''NUM.", "BODY", Uncertain, ""),
        (
            "rule-split",
            r"No \z Ns No \l'1nX'NUM .",
            "BODY",
            Uncertain,
            "",
        ),
        ("rule-rearmed", r"\zX\z\l'1nX'NUM.", "BODY", Uncertain, ""),
        ("overstrike-blank", r"\z\o' 'NUM.", "BODY", Uncertain, ""),
        (
            "overstrike-blank-rearmed",
            r"\zX\z\o' 'NUM.",
            "BODY",
            Uncertain,
            "",
        ),
    ];
    source_matrix(&CASES, "BODY")
}

fn source_matrix(
    templates: &'static [Template],
    ordinary_body: &'static str,
) -> impl Iterator<Item = Case> {
    templates
        .iter()
        .copied()
        .flat_map(move |(name, head, body, disposition, before)| {
            [1_u8, 2, 3].map(move |mask| {
                let mut source = header().to_owned();
                source.push_str(before);
                source.push_str(".Bl -tag -width 6n\n");
                let source_line = u32::try_from(source.lines().count()).unwrap();
                for (bit, number) in [(1, "1"), (2, "2")] {
                    let (head, body) = if mask & bit != 0 {
                        (head, body)
                    } else {
                        ("NUM.", ordinary_body)
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
