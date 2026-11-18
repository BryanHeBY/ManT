//! Accepted native spellings and field scopes at the query boundary.
use super::{check, explain, load};
use mant_ir::EntryKind::{Term, Variable};

#[test]
fn a_native_constant_role_does_not_require_a_bindable_identifier() {
    // mdoc_term.c executes each Dv operand as an ordinary native word.
    // Its spelling is independent of the role received by this declaration.
    for (constant, printed) in [
        ("START", "START"),
        ("0", "0"),
        (r#""BAD X""#, "BAD X"),
        ("O_RDONLY|O_WRONLY", "O_RDONLY|O_WRONLY"),
    ] {
        for first in [false, true] {
            let prefix = if first { "Va FIRST , " } else { "" };
            let mut names = Vec::new();
            if first {
                names.push("FIRST");
            }
            if constant == "START" {
                names.push("START");
            }
            for space_off in [false, true] {
                let gap = if space_off { "," } else { ", " };
                let first_word = if first {
                    format!("FIRST{gap}")
                } else {
                    String::new()
                };
                for (argument, form) in [
                    (".Fa arg ,", "arg"),
                    (".Bo\n.Fa arg\n.Bc ,", "[arg]"),
                    (".Po\n.Fa arg\n.Pc ,", "(arg)"),
                ] {
                    check(
                        &format!("Xo {prefix}Dv {constant} ,\n{argument}\n.Dv STOP\n.Xc"),
                        space_off,
                        if first && constant != "START" {
                            Variable
                        } else {
                            Term
                        },
                        &names,
                        &format!("{first_word}{printed}{gap}{form}{gap}STOP"),
                        &["arg", "STOP"],
                    );
                }
            }
        }
    }
    check(
        "Dv 0 , Fa arg , Dv NULL",
        false,
        Term,
        &[],
        "0, arg, NULL",
        &["0", "arg", "NULL"],
    );
    check(
        "Xo Dv 0 ,\n.Fa const char * ,\n.Dv STOP\n.Xc",
        false,
        Term,
        &[],
        "0, const char *, STOP",
        &["char", "STOP"],
    );
}

#[test]
fn clipped_subjects_retain_the_role_only_while_its_bytes_survive() {
    // term.c::encode1 can overwrite the pending glyph before the word is
    // accepted, and no role is published when none of its text remains.
    check(
        "Xo\n.Dv STA\\zX ,\n.Fa arg ,\n.Dv STOP\n.Xc",
        true,
        Term,
        &[],
        "STA,arg,STOP",
        &["STA", "STAX", "arg", "STOP"],
    );
    check(
        "Xo\n.Dv \\zX\n.Fa arg ,\n.Dv STOP\n.Xc",
        true,
        Term,
        &["arg", "STOP"],
        "arg,STOP",
        &["X"],
    );
}

#[test]
fn only_independent_fields_bind_to_the_definition() {
    // CVS mdoc_term.c::termp_fa_pre renders a regular Fa outside Fo.
    // Inside Fo the same spelling is processed as a formal argument.
    for (head, names, form, arguments) in [
        ("Fa field", &["field"][..], "field", &[][..]),
        (
            "Fa field , Fa other",
            &["field", "other"],
            "field, other",
            &[],
        ),
        ("Fa field , Va var", &["field", "var"], "field, var", &[]),
        ("Ft int , Fa field", &["field"], "int, field", &[]),
        (
            "Va variable , Fa field",
            &["variable", "field"],
            "variable, field",
            &[],
        ),
        (
            r#"Fa "int field" , Fa other"#,
            &["other"],
            "int field, other",
            &["field"],
        ),
        (
            "Fn first , Fa arg , Fn second",
            &["first", "second"],
            "first(), arg, second()",
            &["arg"],
        ),
        (
            "Xo\n.Fo invoke\n.Fa field other\n.Fc\n.Xc",
            &["invoke"],
            "invoke(field, other)",
            &["field", "other"],
        ),
        (
            "Xo\n.Eo <<\n.Fo call\n.Fa hidden\n.Fc\n.Ec >> ,\n.Fa outer\n.Xc",
            &["call", "outer"],
            "<<call(hidden)>>, outer",
            &["hidden"],
        ),
        (
            "Xo\n.Fo first\n.Fa hidden\n.Fc ,\n.Fa last\n.Xc",
            &["first", "last"],
            "first(hidden), last",
            &["hidden"],
        ),
        (
            "Xo\n.Bo\n.Fn call hidden\n.Bc ,\n.Fa outer\n.Xc",
            &["call", "outer"],
            "[call(hidden)], outer",
            &["hidden"],
        ),
        (
            "Xo\n.Eo <<\n.Bo\n.Fo call\n.Fa hidden\n.Fc\n.Bc ,\n.Fa middle\n.Ec >> ,\n.Fa outer\n.Xc",
            &["call", "middle", "outer"],
            "<<[call(hidden)], middle>>, outer",
            &["hidden"],
        ),
    ] {
        check(head, false, Term, names, form, arguments);
    }

    for (section, body) in [
        ("DESCRIPTION", ".Fa field , Fa other\n"),
        (
            "DESCRIPTION",
            ".Bl -tag -width Ds\n.It Fa owned\n.Fa field , Fa other\nOWNERBODY.\n.El\n",
        ),
        ("SYNOPSIS", ".Fo invoke\n.Fa field other\n.Fc\n"),
    ] {
        let query = load(&format!(
            ".Dd November 18, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd checker\n.Sh {section}\n{body}"
        ));
        for field in ["field", "other"] {
            assert_eq!(
                explain(&query, field).counts.direct_entry.total,
                0,
                "{body}"
            );
        }
        let rendered = mant_render::render_query_text(&query);
        assert!(
            rendered.contains("field") && rendered.contains("other"),
            "{body}"
        );
    }
}

#[test]
fn complete_native_delimiters_are_matched_by_scope_and_accepted_range() {
    // mdoc_macro.c::blk_part_exp/blk_exp_close allocate Eo's authored
    // HEAD/TAIL. termp_eo_pre/post only adjust spacing around them.
    for (left, right, form) in [
        ("<<", ">>", "<<arg>>"),
        ("xx", "yy", "xxargyy"),
        ("甲乙", "丙丁", "甲乙arg丙丁"),
        ("BEGIN", "!!!", "BEGINarg!!!"),
        ("<", ">", "<arg>"),
        (r"\&(", r"\&)", "(arg)"),
        (r#""L,R""#, r#""X|Y""#, "L,RargX|Y"),
        (
            r#""LEFT SIDE""#,
            r#""RIGHT SIDE""#,
            "LEFT SIDEargRIGHT SIDE",
        ),
        (r"\fB<\fP<", r"\fI>\fP>", "<<arg>>"),
    ] {
        for space_off in [false, true] {
            let gap = if space_off { "," } else { ", " };
            check(
                &format!("Xo Dv START ,\n.Eo {left}\n.Fa arg\n.Ec {right} ,\n.Dv STOP\n.Xc"),
                space_off,
                Term,
                &["START"],
                &format!("START{gap}{form}{gap}STOP"),
                &["arg", "STOP"],
            );
        }
    }

    for (head, names, form, arguments) in [
        (
            "Xo\n.Eo <<\n.Dv START ,\n.Fa arg ,\n.Dv STOP\n.Ec >> ,\n.Va outside\n.Xc",
            &["START", "outside"][..],
            "<<START, arg, STOP>>, outside",
            &["arg", "STOP"][..],
        ),
        (
            "Xo Dv START ,\n.Eo BEGIN\n.Eo NEST\n.Fa arg\n.Ec leaf\n.Ec END ,\n.Dv STOP\n.Xc",
            &["START"],
            "START, BEGINNESTargleafEND, STOP",
            &["arg", "STOP"],
        ),
        (
            "Xo Dv START\n.Eo <<\n.Fa arg\n.Ec >> ,\n.Dv STOP\n.Xc",
            &["START", "STOP"],
            "START <<arg>>, STOP",
            &["arg"],
        ),
    ] {
        check(head, false, Term, names, form, arguments);
    }
}

#[test]
fn partial_and_unpaired_delimiters_cannot_establish_an_argument_tail() {
    for (left, right, form) in [
        // Bare punctuation is not stored in Eo's HEAD/TAIL by CVS.
        ("(", ")", "(arg)"),
        (r"<\z<", ">>", "<arg>>"),
        ("<<", r">\zX", "<<arg>"),
        ("<<", r"\&", "<<arg"),
        (r"\&", "zz", "argzz"),
        (r#""L\p R""#, "END", "L\nRargEND"),
        ("BEGIN", r#""E\p ND""#, "BEGINargE\nND"),
    ] {
        check(
            &format!("Xo Dv START ,\n.Eo {left}\n.Fa arg\n.Ec {right} ,\n.Dv STOP\n.Xc"),
            false,
            Term,
            &["START", "STOP"],
            &format!("START, {form}, STOP"),
            &["arg"],
        );
    }
    // These delimiters occupy the edges of one lexical group, but the
    // fixed tree puts them in two different Eo blocks with unpaired ends.
    check(
        "Xo Dv START ,\n.Eo <<\n.Fa arg\n.Ec ,\n.Eo\n.Fa spare\n.Ec >> ,\n.Dv STOP\n.Xc",
        false,
        Term,
        &["START", "STOP"],
        "START, <<arg, spare>>, STOP",
        &["arg", "spare"],
    );
    check(
        "Xo Dv START ,\n.Eo <<\n.Fa arg\n.Ec >>\n.Eo <<\n.Fa spare\n.Ec >> ,\n.Dv STOP\n.Xc",
        false,
        Term,
        &["START", "STOP"],
        "START, <<arg>> <<spare>>, STOP",
        &["arg", "spare"],
    );
    // In the pinned tree all three still own BODY/Fa even though neither
    // delimiter is visible. The matching direct Fa is covered above.
    for group in [
        ".Eo\n.Fa arg\n.Ec ,",
        ".Eo \\&\n.Fa arg\n.Ec \\& ,",
        ".Eo \\zX\n.Fa arg\n.Ec \\& ,",
    ] {
        check(
            &format!("Xo Dv START ,\n{group}\n.Dv STOP\n.Xc"),
            false,
            Term,
            &["START", "STOP"],
            "START, arg, STOP",
            &["arg"],
        );
    }
}

#[test]
fn unresolved_enclosures_cannot_suppress_fields_in_an_outer_scope() {
    for (head, form, names, arguments) in [
        (
            "Xo\n.Eo \\&\n.Dv START\n.Ec \\& ,\n.Fa field\n.Xc",
            "START, field",
            &["START", "field"][..],
            &[][..],
        ),
        (
            "Xo\n.Eo <<\n.Dv START\n.Ec >> ,\n.Fa field\n.Xc",
            "<<START>>, field",
            &["START"],
            &["field"],
        ),
        (
            "Xo\n.Eo \\&\n.Fn call hidden\n.Ec \\& ,\n.Fa field\n.Xc",
            "call(hidden), field",
            &["call", "field"],
            &["hidden"],
        ),
        (
            "Xo\n.Eo <<\n.Fn call hidden\n.Ec >> ,\n.Fa field\n.Xc",
            "<<call(hidden)>>, field",
            &["call", "field"],
            &["hidden"],
        ),
    ] {
        check(head, false, Term, names, form, arguments);
    }
}
