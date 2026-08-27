use std::fmt::Write as _;

pub(super) struct Case {
    pub(super) name: &'static str,
    pub(super) iterations: u32,
    pub(super) generate: fn() -> String,
}

pub(super) fn generated_cases() -> [Case; 7] {
    [
        Case {
            name: "small",
            iterations: 1_000,
            generate: || generated_manual(100),
        },
        Case {
            name: "medium",
            iterations: 100,
            generate: || generated_manual(1_000),
        },
        Case {
            name: "large",
            iterations: 10,
            generate: || generated_manual(10_000),
        },
        Case {
            name: "roff-macros",
            iterations: 100,
            generate: || generated_macro_manual(1_000),
        },
        Case {
            name: "mdoc-inline",
            iterations: 100,
            generate: || generated_mdoc(1_000),
        },
        Case {
            name: "tbl-heavy",
            iterations: 100,
            generate: || generated_tables(200),
        },
        Case {
            name: "eqn-heavy",
            iterations: 100,
            generate: || generated_equations(500),
        },
    ]
}

pub(super) fn generated_manual(paragraphs: usize) -> String {
    let mut source = String::with_capacity(paragraphs * 64);
    source.push_str(".TH TRANSFER 1\n.SH NAME\ntransfer \\- generated benchmark\n");
    for index in 0..paragraphs {
        writeln!(
            source,
            ".PP\nparagraph {index} carries stable visible text and \\fBstyle\\fR."
        )
        .expect("writing into String is infallible");
    }
    source
}

fn generated_macro_manual(invocations: usize) -> String {
    let mut source = String::with_capacity(invocations * 48);
    source.push_str(
        ".de BX\n.PP\n\\$1 carries \\fBmacro-expanded\\fR text.\n..\n.TH MACROS 1\n.SH NAME\nmacros \\- generated benchmark\n",
    );
    for index in 0..invocations {
        writeln!(source, ".BX invocation-{index}").expect("writing into String is infallible");
    }
    source
}

fn generated_mdoc(paragraphs: usize) -> String {
    let mut source = String::with_capacity(paragraphs * 72);
    source.push_str(
        ".Dd August 27, 2026\n.Dt BENCH 1\n.Os\n.Sh NAME\n.Nm bench\n.Nd generated benchmark\n.Sh DESCRIPTION\n",
    );
    for index in 0..paragraphs {
        writeln!(
            source,
            ".Pp\n.Em paragraph-{index}\nuses .Fl f with .Ar value and references .Xr printf 3 ."
        )
        .expect("writing into String is infallible");
    }
    source
}

fn generated_tables(tables: usize) -> String {
    let mut source = String::with_capacity(tables * 160);
    source.push_str(".TH TABLES 1\n.SH NAME\ntables \\- generated benchmark\n");
    for index in 0..tables {
        writeln!(
            source,
            ".TS\nbox tab(:);\nl l l.\nname:value:description\nrow-{index}:42:stable table text\n.TE"
        )
        .expect("writing into String is infallible");
    }
    source
}

fn generated_equations(equations: usize) -> String {
    let mut source = String::with_capacity(equations * 96);
    source.push_str(".TH EQUATIONS 1\n.SH NAME\nequations \\- generated benchmark\n");
    for index in 0..equations {
        writeln!(
            source,
            ".PP\nequation {index}:\n.EQ\nx sub {index} + sqrt {{ y sup 2 }}\n.EN"
        )
        .expect("writing into String is infallible");
    }
    source
}
