use super::{check, check_query, explain, load};
use mant_ir::EntryKind;

const HEADER: &str = ".Dd November 18, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd checker\n";

#[test]
fn parser_reopened_inline_macros_keep_only_their_own_annotated_names() {
    // mdoc_macro.c::in_line closes an ELEM at an opening delimiter and
    // reopens the same call for subsequent operands, as shown by -Ttree.
    for (head, names, form, other) in [
        (
            "Dv BIOCGBLEN ( u_int )",
            &["BIOCGBLEN"][..],
            "BIOCGBLEN (u_int)",
            &["u_int"][..],
        ),
        (
            "Dv BIOCSRTIMEOUT , BIOCGRTIMEOUT ( struct timeval )",
            &["BIOCSRTIMEOUT", "BIOCGRTIMEOUT"][..],
            "BIOCSRTIMEOUT, BIOCGRTIMEOUT (struct timeval)",
            &["struct", "timeval"][..],
        ),
        (
            "Er EINVAL ( invalid input )",
            &["EINVAL"][..],
            "EINVAL (invalid input)",
            &["invalid", "input"][..],
        ),
        (
            "Dv GET ( Dv NEXT )",
            &[][..],
            "GET (NEXT)",
            &["GET", "NEXT"][..],
        ),
    ] {
        check(head, false, EntryKind::Term, names, form, other);
    }
}

#[test]
fn expanded_separate_calls_at_the_same_position_cannot_supply_an_owned_annotation() {
    for (source, names, form, other) in [
        (
            ".Dv BIOCGBLEN ( u_int )",
            &["BIOCGBLEN"][..],
            "BIOCGBLEN (u_int)",
            &["u_int"][..],
        ),
        (
            ".Dv GET ( No \\&\n.Dv NEXT\n.No \\& )",
            &[][..],
            "GET ( NEXT )",
            &["GET", "NEXT"][..],
        ),
    ] {
        let query = load(&format!(
            "{HEADER}.Sh DESCRIPTION\n.de ZZ\n{source}\n..\n.Bl -tag -width Ds\n.It Xo\n.ZZ\n.Xc\nOWNERBODY.\n.El\n"
        ));
        check_query(&query, source, EntryKind::Term, names, form, other);
    }
}

#[test]
fn authored_delimiters_end_the_native_name_even_without_a_space() {
    // mdoc_macro.c::blk_part_exp/blk_exp_close put raw Eo punctuation
    // outside the generated bounds. Escaped endpoints stay in HEAD/TAIL.
    for (head, names, form) in [
        (
            "Xo Dv SYS Ns\n.Eo (\n.Ic alias\n.Ec )\n.Xc",
            &["SYS"][..],
            "SYS(alias)",
        ),
        (
            "Xo Dv SYS Ns\n.Eo \\&(\n.Ic alias\n.Ec \\&)\n.Xc",
            &["SYS"][..],
            "SYS(alias)",
        ),
        (
            "Xo Dv SYS Ns\n.Eo (\n.Ic alias\n.Ec \\&)\n.Xc",
            &["SYS"][..],
            "SYS(alias)",
        ),
        (
            "Xo Dv SYS Ns\n.Eo \\&(\n.Ic alias\n.Ec )\n.Xc",
            &["SYS"][..],
            "SYS(alias)",
        ),
        (
            "Xo Dv SYS Ns\n.Po\n.Ic alias\n.Pc\n.Xc",
            &["SYS"][..],
            "SYS(alias)",
        ),
        (
            "Xo Dv SYS Ns\n.Eo x\n.Ic alias\n.Ec z\n.Xc",
            &[][..],
            "SYSxaliasz",
        ),
        (
            "Xo Dv SYS\n.Eo x\n.Ic alias\n.Ec z\n.Xc",
            &["SYS"][..],
            "SYS xaliasz",
        ),
        (
            "Xo Dv SYS Ns\n.Eo (\n.Ic alias\n.Ec\n.Xc",
            &[][..],
            "SYS(alias",
        ),
        (
            "Xo Dv SYS Ns\n.Eo \\&(\n.Ic alias\n.Ec \\&]\n.Xc",
            &[][..],
            "SYS(alias]",
        ),
        (
            "Xo Dv SYS Ns\n.Eo \\&(\n.Ic alias\n.Ec z\n.Xc",
            &[][..],
            "SYS(aliasz",
        ),
        (
            "Xo Dv SYS Ns\n.Eo (\n.Ic alias\n.Ec \\&]\n.Xc",
            &[][..],
            "SYS(alias]",
        ),
        (
            "Xo Dv SYS Ns\n.Eo (more\n.Ic alias\n.Ec ]tail\n.Xc",
            &[][..],
            "SYS(morealias]tail",
        ),
        (
            "Xo Dv SYS Ns\n.Eo (more\n.Ic alias\n.Ec tail)\n.Xc",
            &["SYS"][..],
            "SYS(morealiastail)",
        ),
        (
            "Xo Dv SYS Ns\n.Eo <<\n.Ic alias\n.Ec >>\n.Xc",
            &["SYS"][..],
            "SYS<<alias>>",
        ),
        (
            "Xo Dv SYS Ns\n.Eo <<\n.Ic alias\n.Ec >\n.Xc",
            &[][..],
            "SYS<<alias>",
        ),
    ] {
        check(head, false, EntryKind::Term, names, form, &["alias"]);
    }
}

#[test]
fn environment_annotations_require_an_actual_word_boundary() {
    // mdoc_term.c::termp_quote_pre/post and termp_ns_pre emit real bounds
    // whether there is a blank before Pq or Ns suppresses it.
    for (head, names, form, unbound) in [
        (
            "Ev HOME Pq Em other",
            &["HOME"][..],
            "HOME (other)",
            &["other"][..],
        ),
        (
            "Ev HOME Pq No other",
            &["HOME"][..],
            "HOME (other)",
            &["other"][..],
        ),
        (
            "Ev HOME Pq Ev other",
            &["HOME"][..],
            "HOME (other)",
            &["other"][..],
        ),
        (
            "Ev HOME Ns Pq Em other",
            &["HOME"][..],
            "HOME(other)",
            &["other"][..],
        ),
        (
            "Ev HOME Ns Ar OTHER",
            &[][..],
            "HOMEOTHER",
            &["HOME", "OTHER"][..],
        ),
        (
            "Ev HOME ( Em other",
            &[][..],
            "HOME (other",
            &["HOME", "other"][..],
        ),
        (
            "Xo Ev HOME\n.Po\n.Em other\n.Pc\n.No tail\n.Xc",
            &[][..],
            "HOME (other) tail",
            &["HOME", "other", "tail"][..],
        ),
        (
            "Ev HOME Ns Pq No other",
            &["HOME"][..],
            "HOME(other)",
            &["other"][..],
        ),
        (
            "Ev BASH_FUNC_home()",
            &["BASH_FUNC_home()"][..],
            "BASH_FUNC_home()",
            &["BASH_FUNC_home"][..],
        ),
    ] {
        check_environment(head, names, form, unbound);
    }
}

#[test]
fn variable_annotations_require_an_actual_word_boundary() {
    for (head, names, form, unbound) in [
        (
            "Va local Pq No other",
            &["local"][..],
            "local (other)",
            &["other"][..],
        ),
        (
            "Va local Pq Em other",
            &["local"][..],
            "local (other)",
            &["other"][..],
        ),
        (
            "Va local Pq Vt int",
            &["local"][..],
            "local (int)",
            &["int"][..],
        ),
        (
            "Va local Ns Pq No other",
            &["local"][..],
            "local(other)",
            &["other"][..],
        ),
        (
            "Va local Ns Ar OTHER",
            &[][..],
            "localOTHER",
            &["local", "OTHER"][..],
        ),
        (
            "Va local ( Em other",
            &[][..],
            "local (other",
            &["local", "other"][..],
        ),
        (
            "Xo Va local\n.Po\n.Em other\n.Pc\n.No tail\n.Xc",
            &[][..],
            "local (other) tail",
            &["local", "other", "tail"][..],
        ),
    ] {
        let kind = if names.is_empty() {
            EntryKind::Term
        } else {
            EntryKind::Variable
        };
        check(head, false, kind, names, form, unbound);
    }
}

#[test]
fn environment_annotation_uses_the_same_grammar_for_each_alias() {
    const ANNOTATION: &str = ".Ev HOME\n.Po\n.Em other\n.Pc";
    for (other, form) in [
        (".Ev PATH Ar value", "PATH value"),
        (".Ev PATH No = Ar value", "PATH = value"),
    ] {
        for (first, second, names, expected) in [
            (
                ANNOTATION,
                other,
                ["HOME", "PATH"],
                format!("HOME (other), {form}"),
            ),
            (
                other,
                ANNOTATION,
                ["PATH", "HOME"],
                format!("{form}, HOME (other)"),
            ),
        ] {
            let head = format!("Xo\n{first} ,\n{second}\n.Xc");
            check_environment(&head, &names, &expected, &["other", "value"]);
        }
    }
}

fn check_environment(head: &str, names: &[&str], form: &str, unbound: &[&str]) {
    let query = load(&format!(
        "{HEADER}.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It {head}\nOWNERBODY.\n.El\n"
    ));
    check_query(
        &query,
        head,
        EntryKind::EnvironmentVariable,
        names,
        form,
        unbound,
    );
}

#[test]
fn annotations_outside_definition_heads_do_not_create_definitions() {
    let query = load(&format!(
        "{HEADER}.Sh SYNOPSIS\n.Ev HOME Pq Em synopsis\n.Va local Pq No synopsis\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It entry\n.Ev HOME Pq Em body\n.Va local Pq No body\n.El\n"
    ));
    for (name, snippet) in [("HOME", "HOME (body)"), ("local", "local (body)")] {
        assert_eq!(explain(&query, name).counts.direct_entry.total, 0, "{name}");
        assert!(mant_render::render_query_text(&query).contains(snippet));
    }
}
