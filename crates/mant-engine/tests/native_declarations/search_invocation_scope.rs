//! The executed mdoc enclosure determines which punctuation can separate names.
use super::support::{check_occurrences, explain, load, roundtrip, source};
use mant_ir::{Document, EntryOwner, inline_plain_text};
use mant_protocol::{DocumentResponse, EvidenceClass, QueryExplanation};

const SECTION: &str = ".Sh COMMANDS\n.Sm on\n";
const SECTION_WITH_ES: &str = ".Sh COMMANDS\n.Sm on\n.Es \\[lq] \\[rq]\n";

#[test]
fn punctuation_in_quoted_invocation_arguments_stays_in_scope() {
    // CVS mdoc_term.c termp_quote_pre/post write the marks at the original
    // ELEM/BODY and termp_eo_pre/post execute the author's HEAD/BODY/TAIL.
    // In all these lint-clean inputs, read owns one invocation.
    for separator in ["|", ","] {
        let operands = format!(r"Fl n Ar n {separator} Fl N Ar n Ar vname Ns Sy \&? Ns Ar prompt");
        let alternatives = if separator == "|" {
            "-n n | -N n vname?prompt"
        } else {
            "-n n, -N n vname?prompt"
        };
        for (quote, open, close) in [
            ("Op", "[", "]"),
            ("Bq", "[", "]"),
            ("Pq", "(", ")"),
            ("Brq", "{", "}"),
            ("Aq", "⟨", "⟩"),
            ("Dq", "“", "”"),
            ("Qq", "\"", "\""),
            ("Sq", "‘", "’"),
            ("Qq Qq", "\"\"", "\"\""),
            ("Dq Qq", "“\"", "\"”"),
        ] {
            let head = format!(".It Ic read {quote} {operands}");
            let form = format!("read {open}{alternatives}{close}");
            check_scope(SECTION, &head, &form, "read", "-N");
        }
        for (begin, end, open, close) in [
            ("Do", "Dc", "“", "”"),
            ("Qo", "Qc", "\"", "\""),
            ("So", "Sc", "‘", "’"),
            ("Ao", "Ac", "⟨", "⟩"),
            ("Bo", "Bc", "[", "]"),
            ("Bro", "Brc", "{", "}"),
            ("Po", "Pc", "(", ")"),
            (r"Eo \[lq]", r"Ec \[rq]", "“", "”"),
        ] {
            let head = format!(".It Xo\n.Ic read\n.{begin}\n.{operands}\n.{end}\n.Xc");
            let form = format!("read {open}{alternatives}{close}");
            check_scope(SECTION, &head, &form, "read", "-N");
        }
    }
}

#[test]
fn authored_or_overwritten_marks_leave_the_inner_punctuation_scoped() {
    let arguments = r"Fl n Ar n | Fl N Ar n Ar vname Ns Sy \&? Ns Ar prompt";
    for (section, head, form) in [
        // CVS diagnoses only the obsolescence of Es and En.
        (
            SECTION_WITH_ES,
            format!(".It Ic read En {arguments}"),
            "read “-n n | -N n vname?prompt”",
        ),
        (
            SECTION,
            format!(
                ".It Xo\n.Ic read\n.Eo \\[lq]\n.Eo \\[lq]\n.{arguments}\n.Ec \\[rq]\n.Ec \\[rq]\n.Xc"
            ),
            "read ““-n n | -N n vname?prompt””",
        ),
        // CVS term.c::encode1 can overwrite the pending closing quote.
        (
            SECTION,
            format!(".It Xo\n.Ic read\n.Qo\n.{arguments}\\z\n.Qc Ns No X\n.Xc"),
            "read \"-n n | -N n vname?promptX",
        ),
        (
            SECTION,
            format!(".It Xo\n.Ic read\n.Eo \\[lq]\n.{arguments}\n.Ec \\&\n.Xc"),
            "read “-n n | -N n vname?prompt",
        ),
    ] {
        check_scope(section, &head, form, "read", "-N");
    }
}

#[test]
fn actual_separators_still_name_independent_options() {
    // The fixed CVS tree places the comma inside the complete outer
    // wrapper or after the original BODY. A missing mark cannot move it.
    for (section, head, form) in [
        (SECTION, r".It Qq Sy \&? Ns Ar RE , Fl q", "\"?RE, -q\""),
        (
            SECTION,
            ".It Xo\n.Qo Sy \\&? Ns Ar RE\n.Qc , Fl q\n.Xc",
            "\"?RE\", -q",
        ),
        (
            SECTION,
            ".It Xo\n.Eo \\[lq]\n.Sy \\&? Ns Ar RE\n.Ec \\[rq] , Fl q\n.Xc",
            "“?RE”, -q",
        ),
        (
            SECTION,
            ".It Xo\n.Qo\n.Sy \\&? Ns Ar RE\\z\n.Qc , Fl q\n.Xc",
            "\"?RE, -q",
        ),
        (
            SECTION,
            ".It Xo\n.Eo \\[lq]\n.Sy \\&? Ns Ar RE\n.Ec \\& , Fl q\n.Xc",
            "“?RE, -q",
        ),
        (
            SECTION,
            ".It Xo\n.Qo Sy \\&? Ns Ar RE\n.Qc ,\n.Qo Fl q\n.Qc\n.Xc",
            "\"?RE\", \"-q\"",
        ),
        (
            SECTION_WITH_ES,
            r".It En Sy \&? Ns Ar RE , Fl q",
            "“?RE, -q”",
        ),
    ] {
        check_scope(section, head, form, "-q", "RE");
    }
}

fn check_scope(section: &str, head: &str, form: &str, name: &str, parameter: &str) {
    let input = source("", section, &format!("{head}\n"));
    let content = load(&input);
    let document = content.document.as_ref().unwrap();
    let first = super::definitions(document)[0];
    assert_eq!(inline_plain_text(&first.terms[0]), form, "{input}");
    assert_eq!(
        EntryOwner::Definition(first).validated_names().unwrap(),
        [name],
        "{input}"
    );
    assert!(first.description.is_empty(), "{input}");
    assert_eq!(
        Document::from(roundtrip(&DocumentResponse::from(document))),
        *document,
        "{input}"
    );

    let result: QueryExplanation = roundtrip(&explain(&content, name));
    assert_eq!(result.counts.direct_entry.total, 1, "{input}");
    let hit = result
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(hit.source, first.source, "{input}");
    let start = form[..form.find(name).unwrap()].chars().count();
    let range = start..start + name.chars().count();
    check_occurrences(hit, name, std::slice::from_ref(&range));

    let result: QueryExplanation = roundtrip(&explain(&content, parameter));
    assert_eq!(result.counts.direct_entry.total, 0, "{input}");
    assert!(result.supports.is_empty(), "{input}");
}
