//! Complete independent sources, replayed before consumer assertions.
//!
//! Pinned `roff_term.c::roff_term_pre_mc` only flushes nonempty input and clears
//! NOSPACE after that flush. term.c:233-237 restores minbl from trailspace;
//! ordinary prose does not acquire an irrevocable extra field separator.
//! ASCII/UTF-8/HTML/tree/lint all succeeded for each exact source.

pub struct Case {
    pub id: &'static str,
    pub source: &'static str,
    pub row: &'static str,
    pub rejected_row: &'static str,
}

pub const CASES: [Case; 2] = [
    Case {
        id: "empty-word",
        source: ".Dd November 5, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd spacing cursor review\n.Sh DESCRIPTION\nORIGIN\n.br\nFIRST\n.mc\n.No \"\"\n.mc\nSECOND\n.br\nTAIL\n.Sh NEXT\nEND\n",
        row: "FIRST SECOND",
        rejected_row: "FIRST  SECOND",
    },
    Case {
        id: "joined-empty-word",
        source: ".Dd November 5, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd spacing cursor review\n.Sh DESCRIPTION\nORIGIN\n.br\n.No FIRST\n.mc\n.Sm off\n.Xo Ns\n.No \"\"\n.Xc\n.mc\n.Sm on\nSECOND\n.br\nTAIL\n.Sh NEXT\nEND\n",
        row: "FIRSTSECOND",
        rejected_row: "FIRST SECOND",
    },
];
