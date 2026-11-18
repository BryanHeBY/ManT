//! Attached native declaration text and accepted word boundaries.
use super::{check, explain, load, roundtrip};
use mant_ir::EntryKind::Term;
use mant_protocol::{ContentSelector, EvidenceClass};

#[test]
fn native_and_literal_enclosures_keep_the_declared_symbol() {
    // CVS termp_quote_pre/post emits the Pq marks. mdoc_macro.c::dword
    // instead flags standalone authored parentheses as native delimiters.
    for (head, subject, form, annotation) in [
        ("Dv GET", "GET", "GET", "config"),
        ("Dv GET Fa val", "GET", "GET val", "val"),
        ("Dv GET Pq Ar val", "GET", "GET (val)", "val"),
        ("Dv GET Pq Vt int", "GET", "GET (int)", "int"),
        (
            "Dv NGM_DEFLATE_CONFIG Pq Ic config",
            "NGM_DEFLATE_CONFIG",
            "NGM_DEFLATE_CONFIG (config)",
            "config",
        ),
        (
            "Dv ARCHIVE_ENTRY_ACL_READ ( Sy r )",
            "ARCHIVE_ENTRY_ACL_READ",
            "ARCHIVE_ENTRY_ACL_READ (r)",
            "r",
        ),
        ("Dv GET Pq Cm config", "GET", "GET (config)", "config"),
        ("Dv GET [ Sy r ]", "GET", "GET [r]", "r"),
        (
            "Dv GET ( No normal text )",
            "GET",
            "GET (normal text)",
            "normal",
        ),
        ("Dv GET Ns ( Sy r )", "GET", "GET(r)", "r"),
        (
            "Dv GET ( Sy r ( Em inner ) )",
            "GET",
            "GET (r (inner))",
            "inner",
        ),
        (
            "Dv GET ( Sy note Pq Ic config )",
            "GET",
            "GET (note (config))",
            "config",
        ),
        (
            "Er 1 EFAIL ( Em denial )",
            "EFAIL",
            "1 EFAIL (denial)",
            "denial",
        ),
    ] {
        check(head, false, Term, &[subject], form, &[annotation]);
    }
}

#[test]
fn supplementary_bounds_cannot_be_inferred_from_spelling_or_hide_subjects() {
    for (head, form) in [
        (r#"Dv GET Ns Sy "(r)""#, "GET(r)"),
        ("Dv GET ( Sy r", "GET (r"),
        ("Dv GET ) Sy r", "GET) r"),
        ("Dv GET ( Sy r ]", "GET (r]"),
        ("Dv GET ( Dv NEXT )", "GET (NEXT)"),
        ("Dv GET Pq Dv NEXT", "GET (NEXT)"),
        ("Dv GET Pq Ic config , Dv NEXT", "GET (config, NEXT)"),
    ] {
        check(head, false, Term, &[], form, &["GET", "NEXT", "r"]);
    }
    for (head, form) in [
        ("Dv GET ( Sy r ) , Dv NEXT", "GET (r), NEXT"),
        (
            "Xo Dv GET\n.Po\n.Ic config\n.Pc ,\n.Dv NEXT\n.Xc",
            "GET (config), NEXT",
        ),
    ] {
        check(head, false, Term, &["GET", "NEXT"], form, &["config", "r"]);
    }
}

#[test]
fn field_groups_keep_argument_state_across_attached_prose_and_punctuation() {
    // CVS closes Fa before the literal period or Em. That syntax changes
    // the displayed argument, while Fa still starts the same group.
    for (head, form) in [
        ("Dv GET , Fa a , Dv STOP", "GET, a, STOP"),
        ("Dv GET , Fa a . , Dv STOP", "GET, a., STOP"),
        ("Dv GET , Fa a Em note , Dv STOP", "GET, a note, STOP"),
        ("Dv GET , Fa a No note , Dv STOP", "GET, a note, STOP"),
        (r"Dv GET , Fa a No \fBnote , Dv STOP", "GET, a note, STOP"),
        ("Dv GET , Fa a Ic note , Dv STOP", "GET, a note, STOP"),
        ("Dv GET , Fa a Li note , Dv STOP", "GET, a note, STOP"),
        (r#"Dv GET , Fa "a note" , Dv STOP"#, "GET, a note, STOP"),
        ("Dv GET , ( Fa a ) , Dv STOP", "GET, (a), STOP"),
        ("Dv GET , Fa a ( Sy r ) , Dv STOP", "GET, a (r), STOP"),
        (
            "Xo Dv GET ,\n.Po\n.Fa a\n.Em note\n.Pc ,\n.Dv STOP\n.Xc",
            "GET, (a note), STOP",
        ),
        (
            "Xo Dv GET ,\n.Po\n.Fa a\n.No note\n.Pc ,\n.Dv STOP\n.Xc",
            "GET, (a note), STOP",
        ),
    ] {
        check(
            head,
            false,
            Term,
            &["GET"],
            form,
            &["a", "note", "r", "STOP"],
        );
    }
    // One close to the attached group is missing, or the text contains a
    // separate native subject rather than a field or qualified annotation.
    for (head, form) in [
        ("Dv GET , Fa a ( Dv NEXT ) , Dv STOP", "GET, a (NEXT), STOP"),
        (
            "Xo Dv GET ,\n.Fa a\n.Eo\n.Fa b\n.Ec ,\n.Dv STOP\n.Xc",
            "GET, a b, STOP",
        ),
    ] {
        check(
            head,
            false,
            Term,
            &["GET", "STOP"],
            form,
            &["a", "b", "NEXT"],
        );
    }
}

#[test]
fn punctuation_boundaries_follow_the_complete_visible_word() {
    // CVS termp_ns_pre suppresses the actual space. Splitting that one
    // printed word across two fonts still cannot name an identifier prefix.
    for (head, form) in [
        ("Dv SYS Ns Em .cache", "SYS.cache"),
        (r"Dv SYS Ns Em \&. Ns Em cache", "SYS.cache"),
        (r"Dv SYS Ns Em \&. Ns Sy cache", "SYS.cache"),
        ("Dv SYS Ns Em :cache", "SYS:cache"),
        (r"Dv SYS Ns Em \&: Ns Em cache", "SYS:cache"),
    ] {
        check(head, false, Term, &[], form, &["SYS", "cache"]);
    }
    for (head, form) in [
        ("Dv SYS .", "SYS."),
        ("Dv SYS . Em cache", "SYS. cache"),
        (r"Dv SYS Ns Em \&. Em cache", "SYS. cache"),
        (r"Dv SYS Ns Em \&: Em cache", "SYS: cache"),
        ("Dv SYS :", "SYS:"),
    ] {
        check(head, false, Term, &["SYS"], form, &["cache"]);
    }
}

#[test]
fn the_error_number_cannot_join_the_symbol_into_one_word() {
    // CVS termp_sm_pre sets NONOSPACE without changing Er's native TEXT
    // children. An adjacent number prevents the visible name boundary.
    for (head, symbol, spaced, joined) in [
        ("Er 1 EPERM", "EPERM", "1 EPERM", "1EPERM"),
        (r"Er \-1 ERESTART", "ERESTART", "-1 ERESTART", "-1ERESTART"),
    ] {
        check(head, false, Term, &[symbol], spaced, &[]);
        check(head, true, Term, &[], joined, &[symbol]);
    }
    check("Er 1EPERM", false, Term, &[], "1EPERM", &["EPERM"]);
    check(
        r"Er \-1ERESTART",
        false,
        Term,
        &[],
        "-1ERESTART",
        &["ERESTART"],
    );
}

#[test]
fn equivalent_annotations_preserve_separate_body_owners() {
    let query = load(concat!(
        ".Dd November 18, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd checker\n",
        ".Sh DESCRIPTION\n.Bl -tag -width Ds\n",
        ".It Dv GET Pq Ic config\nFIRSTBODY.\n",
        ".It Dv GET ( Sy config )\nSECONDBODY.\n.El\n",
        ".Dv UNOWNED Pq Ic config\n",
    ));
    let result = roundtrip(&explain(&query, "GET"));
    assert_eq!(result.counts.direct_entry.total, 2);
    for (hit, body) in result
        .evidence
        .iter()
        .filter(|hit| hit.class == EvidenceClass::DirectEntry)
        .zip(["FIRSTBODY.", "SECONDBODY."])
    {
        super::super::super::support::check_occurrences(hit, "GET", std::slice::from_ref(&(0..3)));
        let by_id = roundtrip(
            &mant_query::select_excerpt(&query, &[ContentSelector::id(hit.outline.node.id())])
                .unwrap(),
        );
        let by_path =
            mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        assert_eq!(by_id, by_path);
        let rendered = mant_render::render_excerpt_text(&by_id);
        assert!(
            rendered.contains("GET (config)") && rendered.contains(body),
            "{rendered}"
        );
    }
    for name in ["config", "UNOWNED"] {
        assert_eq!(explain(&query, name).counts.direct_entry.total, 0, "{name}");
    }
}
