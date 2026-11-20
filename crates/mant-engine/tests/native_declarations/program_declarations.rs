//! Standalone and contiguous compound names in source-owned definition heads.
use mant_ir::{EntryKind, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceClass};

use super::assignment_and_arguments::check;
use super::support::{check_head, explain, explanation_ranges, load, roundtrip};

const HEADER: &str = ".Dd November 19, 2026\n.Dt TOOL 1\n.Os\n.Sh NAME\n.Nm tool\n.Nd sample\n";
const BODY: &str = "Owner proof.";

fn source(section: &str, head: &str) -> String {
    format!("{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\n{BODY}\n.El\n")
}

#[test]
fn compound_command_names_stop_at_argument_syntax_across_native_words() {
    // mdoc_macro.c::in_line keeps Cm/Ic words under the same scope, and
    // termp_bold_pre prints the same prefix with or without source quotes.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for macro_name in ["Cm", "Ic"] {
            for (argument, visible, name, negative) in [
                (
                    "mount (FILE)",
                    "mount (FILE)",
                    "tool mount",
                    "tool mount (FILE)",
                ),
                (
                    r#""mount (FILE)""#,
                    "mount (FILE)",
                    "tool mount",
                    "tool mount (FILE)",
                ),
                (
                    "mount child",
                    "mount child",
                    "tool mount child",
                    "tool mount",
                ),
                ("mount -p", "mount -p", "tool mount", "tool mount -p"),
                (
                    "mount [FILE]",
                    "mount [FILE]",
                    "tool mount",
                    "tool mount [FILE]",
                ),
            ] {
                let head = format!("Nm tool {macro_name} {argument} Ar path");
                let form = format!("tool {visible} path");
                for source_head in [head.clone(), format!("Xo\n.{head}\n.Xc")] {
                    check(
                        &source(section, &source_head),
                        &form,
                        &[name],
                        &[negative, "path"],
                        BODY,
                        9,
                    );
                }
                let head = format!("Xo\n.{head}\n.br\n.TS\nl.\nCELL\n.TE\n.Xc");
                check_display(section, &head, name, &form, negative);
            }
        }
    }
}

fn check_display(section: &str, head: &str, name: &str, form: &str, negative: &str) {
    let content = load(&source(section, head));
    let items = super::definitions(content.document.as_ref().unwrap());
    let [item] = items.as_slice() else {
        panic!("{head}");
    };
    assert_eq!(item.entry.as_ref().unwrap().kind, EntryKind::Command);
    check_head(item, name, &[name]);
    assert_eq!(inline_plain_text(&item.terms[0]), format!("{form}\nCELL"));
    let result = roundtrip(&explain(&content, name));
    assert_eq!(result.counts.direct_entry.total, 1, "{head}");
    assert!(explanation_ranges::assert_ranges(&result, name) > 0);
    let hit = result
        .evidence
        .iter()
        .find(|item| item.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(hit.source.unwrap().line, 9);
    let excerpt =
        mant_query::select_excerpt(&content, &[ContentSelector::path(hit.outline.path())]).unwrap();
    let visible = mant_render::render_excerpt_text(&excerpt);
    assert!(visible.contains(BODY) && visible.contains("CELL"), "{head}");
    for query in [negative, form, "path", "CELL"] {
        assert_eq!(
            explain(&content, query).counts.direct_entry.total,
            0,
            "{head} {query}"
        );
    }
}

#[test]
fn standalone_names_precede_options_operands_and_enclosed_choices() {
    // CVS termp_nm_pre renders the actual Nm first. Cm in the Brq BODY
    // belongs to an alternative argument rather than the outer prefix.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for program in ["Nm tool", "Nm"] {
            for (tail, form, negatives) in [
                ("Op Fl j", "tool [-j]", &["-j"][..]),
                ("Ar file", "tool file", &["file"][..]),
                ("Brq Cm c | Cm x", "tool {c | x}", &["c", "x"][..]),
            ] {
                check(
                    &source(section, &format!("{program} {tail}")),
                    form,
                    &["tool"],
                    negatives,
                    BODY,
                    9,
                );
            }
        }
    }
}

#[test]
fn detached_or_clipped_invocations_cannot_prove_the_program_alone() {
    // In pinned mdoc_macro.c::in_line, both words of Nm belong to its
    // element. term.c::encode1 writes an overstrike before the remaining
    // Cm spelling, so the surviving prefix is insufficient evidence.
    for (head, visible, negative) in [
        ("Nm tool mount", "tool mount", "tool mount"),
        ("Nm tool Oo Fl d Oc Cm run", "tool [-d] run", "tool run"),
        (
            r"Nm tool Cm \zXmount Op Fl j",
            "tool mount [-j]",
            "tool mount",
        ),
        (r"Nm \zXtool Op Fl j", "tool [-j]", "tool"),
    ] {
        let content = load(&source("DESCRIPTION", head));
        let item = super::definitions(content.document.as_ref().unwrap())[0];
        check_head(item, visible, &[]);
        assert_eq!(
            explain(&content, "tool").counts.direct_entry.total,
            0,
            "{head}"
        );
        assert_eq!(
            explain(&content, negative).counts.direct_entry.total,
            u32::from(visible == negative),
            "{head}"
        );
    }
}

#[test]
fn assignment_groups_bind_independent_native_operands_without_unrelated_siblings() {
    // In mdoc_macro.c::in_line, a bare separator reopens the same macro
    // scope. An explicitly called macro to its right has its own scope.
    for section in ["DESCRIPTION", "OPTIONS", "COMMANDS"] {
        for (macro_name, lhs, rhs, left, right) in [
            ("Fl", "a", "b", "-a", "-b"),
            ("Ev", "RED", "BLUE", "RED", "BLUE"),
            ("Dv", "RED", "BLUE", "RED", "BLUE"),
        ] {
            for (separator, visible) in [(",", ", "), ("|", " | ")] {
                for repeated in [false, true] {
                    let call = if repeated {
                        format!("{macro_name} ")
                    } else {
                        String::new()
                    };
                    let subhead = format!("Sy mode = {macro_name} {lhs} {separator} {call}{rhs}");
                    let value = format!("mode = {left}{visible}{right}");
                    for (before, after) in [("", ""), ("", " , Dv EXTRA"), ("Dv PREFIX , ", "")] {
                        let mut names = Vec::new();
                        if !before.is_empty() {
                            names.push("PREFIX");
                        }
                        names.push("mode");
                        if repeated {
                            names.push(right);
                        }
                        if !after.is_empty() {
                            names.push("EXTRA");
                        }
                        let input = source(section, &format!("{before}{subhead}{after}"));
                        let start = if before.is_empty() { "" } else { "PREFIX, " };
                        let tail = if after.is_empty() { "" } else { ", EXTRA" };
                        let form = format!("{start}{value}{tail}");
                        let mut operands = vec![left, "mode ="];
                        if !repeated {
                            operands.push(right);
                        }
                        check(&input, &form, &names, &operands, BODY, 9);
                    }
                }
            }
        }
    }
}

#[test]
fn program_groups_require_invocation_evidence_at_their_own_scope() {
    // mdoc_macro.c::blk_part_imp keeps the remaining line inside Op/Brq.
    // With explicit Oo/Oc or Bro/Brc the following comma belongs to It.
    for section in ["COMMANDS", "DESCRIPTION"] {
        for (head, form, names, non_names) in [
            (
                "Nm foo Ar arg , Dv SIGINT",
                "foo arg, SIGINT",
                &["foo", "SIGINT"][..],
                &["arg"][..],
            ),
            (
                "Dv SIGINT , Nm foo Ar arg",
                "SIGINT, foo arg",
                &["SIGINT", "foo"],
                &["arg"],
            ),
            (
                "Nm foo Oo Fl a Oc , Nm bar Op Fl b",
                "foo [-a], bar [-b]",
                &["foo", "bar"],
                &["-a", "-b"],
            ),
            (
                "Nm foo Op Fl a , Nm bar Op Fl b",
                "foo [-a, bar [-b]]",
                &["foo"],
                &["bar", "-b"],
            ),
            (
                "Nm foo Bro Cm c | Cm x Brc , Dv SIGINT",
                "foo {c | x}, SIGINT",
                &["foo", "SIGINT"],
                &["c", "x"],
            ),
            (
                "Nm foo No version , Nm bar Ar arg",
                "foo version, bar arg",
                &["bar"],
                &["foo", "version"],
            ),
            (
                "Nm foo Ar arg , Nm bar No version",
                "foo arg, bar version",
                &["foo"],
                &["bar", "version"],
            ),
            ("Nm a , Nm b", "a, b", &["a", "b"], &[]),
            ("Nm foo | Nm bar", "foo | bar", &[], &["foo", "bar"]),
            (
                "Nm tool Ar arg | Nm next Ar arg",
                "tool arg | next arg",
                &[],
                &["tool", "next"],
            ),
            (
                "Nm HAMMER No version 2-",
                "HAMMER version 2-",
                &[],
                &["HAMMER"],
            ),
            (
                "Nm HAMMER Sy version 2-",
                "HAMMER version 2-",
                &[],
                &["HAMMER"],
            ),
            (
                "Nm HAMMER Op No version",
                "HAMMER [version]",
                &[],
                &["HAMMER"],
            ),
            (
                "Nm HAMMER No example Ar path",
                "HAMMER example path",
                &[],
                &["HAMMER", "path"],
            ),
        ] {
            assert_program_group(section, head, form, names, non_names);
        }
    }
}

#[test]
fn program_pipes_respect_authored_and_generated_argument_groups() {
    // mdoc.c::mdoc_isdelim recognizes the Roman-font pipe too. The pinned
    // terminal and HTML agree on both literal and generated delimiters.
    for section in ["COMMANDS", "DESCRIPTION"] {
        for (open, close, rendered_open, rendered_close) in [
            ("(", ")", "(", ")"),
            ("[", "]", "[", "]"),
            ("( [", "] )", "([", "])"),
            ("Po", "Pc", "(", ")"),
            ("Oo", "Oc", "[", "]"),
            ("Po Oo", "Oc Pc", "([", "])"),
        ] {
            let head = format!("Nm alpha Cm dump {open} Ar a | Nm inner {close}");
            let form = format!("alpha dump {rendered_open}a | inner{rendered_close}");
            for (prefix, suffix, before, after, names) in [
                ("", "", "", "", &["alpha dump"][..]),
                ("Nm gamma , ", "", "gamma, ", "", &["gamma", "alpha dump"]),
                ("", " , Nm gamma", "", ", gamma", &["alpha dump", "gamma"]),
            ] {
                assert_program_group(
                    section,
                    &format!("{prefix}{head}{suffix}"),
                    &format!("{before}{form}{after}"),
                    names,
                    &["inner"],
                );
            }
            assert_program_group(
                section,
                &format!("{head} | Nm beta Cm read"),
                &format!("{form} | beta read"),
                &[],
                &["alpha dump", "inner", "beta read"],
            );
        }
        for (rhs, visible) in [
            ("Nm beta Cm read", "beta read"),
            ("( Nm beta Cm read )", "(beta read)"),
            ("Pq Nm beta Cm read", "(beta read)"),
            ("Po Nm beta Cm read Pc", "(beta read)"),
        ] {
            for pipe in ["|", r"\fR|\fP"] {
                assert_program_group(
                    section,
                    &format!("Nm alpha Cm dump {pipe} {rhs}"),
                    &format!("alpha dump | {visible}"),
                    &[],
                    &["alpha dump", "beta read"],
                );
            }
        }
    }
}

#[test]
fn authored_group_edges_preserve_qualified_program_names_and_aliases() {
    // CVS mdoc_isdelim() marks bare punctuation, while mdoc_macro.c::dword
    // can leave an escaped edge or an opening bracket inside Ar. The
    // accepted spelling and argument roles still prove the same invocation.
    for section in ["COMMANDS", "DESCRIPTION"] {
        for (head, form, names) in [
            (
                r"Nm alpha Cm dump Ar path Ns [ Ar index ]",
                "alpha dump path[index]",
                &["alpha dump"][..],
            ),
            (
                r"Nm alpha Cm dump Ar path Ns \&[ Ns Ar index ]",
                "alpha dump path[index]",
                &["alpha dump"][..],
            ),
            (
                r"Nm alpha Cm dump Ar path Ns [ Ar index Ns \&]",
                "alpha dump path[index]",
                &["alpha dump"][..],
            ),
            (
                r"Nm alpha Cm dump Ar path[ Ns Ar index ]",
                "alpha dump path[index]",
                &["alpha dump"][..],
            ),
            ("Nm alpha [ Ar file ]", "alpha [file]", &["alpha"][..]),
            ("Nm alpha Oo Ar file Oc", "alpha [file]", &["alpha"][..]),
            ("Nm alpha ( Fl a )", "alpha (-a)", &["alpha"][..]),
            ("Nm alpha [ No filename ]", "alpha [filename]", &[][..]),
            (
                r"Nm alpha Cm dump [ Ar a , Nm inner Ns \&] , Nm gamma",
                "alpha dump [a, inner], gamma",
                &["alpha dump", "gamma"][..],
            ),
            (
                r"Nm alpha Cm dump [ Ar a | Nm inner Ns \&]",
                "alpha dump [a | inner]",
                &["alpha dump"][..],
            ),
            (
                r"Nm alpha Cm dump [ Ar a | Nm inner Ns \&] | Nm beta Cm read",
                "alpha dump [a | inner] | beta read",
                &[][..],
            ),
            (
                "Nm alpha Cm dump [ Ar a | Nm inner",
                "alpha dump [a | inner",
                &[][..],
            ),
        ] {
            for head in [head.to_owned(), format!("Xo\n.{head}\n.Xc")] {
                let non_names = ["index", "file", "filename", "inner", "beta read"]
                    .into_iter()
                    .filter(|operand| form.contains(operand))
                    .collect::<Vec<_>>();
                check(&source(section, &head), form, names, &non_names, BODY, 9);
            }
        }
    }
}

#[test]
fn parameter_internal_brackets_do_not_close_the_invocation() {
    // mdoc_isdelim() is called on an entire token. CVS keeps these
    // paired brackets in one Ar, including inside an explicit Oo BODY.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (head, form, names, argument) in [
            (
                "Nm alpha Ar path[index]",
                "alpha path[index]",
                &["alpha"][..],
                "path[index]",
            ),
            (
                "Nm alpha Ar table[row][column]",
                "alpha table[row][column]",
                &["alpha"][..],
                "table[row][column]",
            ),
            (
                r#"Nm alpha Op Ar "file(s)""#,
                "alpha [file(s)]",
                &["alpha"][..],
                "file(s)",
            ),
            (
                r"Nm alpha Cm dump Oo Ar path\&[index\&] | Nm beta Oc",
                "alpha dump [path[index] | beta]",
                &["alpha dump"][..],
                "path[index]",
            ),
            (
                "Nm gamma , Nm alpha Cm dump Oo Ar map[(index)] | Nm beta Oc",
                "gamma, alpha dump [map[(index)] | beta]",
                &["gamma", "alpha dump"][..],
                "map[(index)]",
            ),
            (
                "Nm alpha Ar path[index] | Nm beta , Nm gamma",
                "alpha path[index] | beta, gamma",
                &["gamma"][..],
                "path[index]",
            ),
            (
                "Nm alpha Cm dump Ar path[ Ns Ar index ]",
                "alpha dump path[index]",
                &["alpha dump"][..],
                "path[index]",
            ),
        ] {
            for head in [head.to_owned(), format!("Xo\n.{head}\n.Xc")] {
                let rejected = [argument, "beta"]
                    .into_iter()
                    .filter(|name| form.contains(name))
                    .collect::<Vec<_>>();
                check(&source(section, &head), form, names, &rejected, BODY, 9);
            }
        }
    }
}

#[test]
fn surviving_native_ink_owns_the_definition_name_and_its_body() {
    // term.c::encode1 keeps both author glyphs when it arms a backspace.
    // The final HORIZ overwrites an earlier backspace, exposing B.
    let page = source("DESCRIPTION", r#"No "\zMA\zBC" No "\h'-3n'T""#);
    let content = load(&page);
    let items = super::definitions(content.document.as_ref().unwrap());
    let [item] = items.as_slice() else {
        panic!("{page}")
    };
    check_head(item, "ABTC", &["ABTC"]);
    let result = roundtrip(&explain(&content, "ABTC"));
    assert_eq!(result.counts.direct_entry.total, 1);
    assert!(explanation_ranges::assert_ranges(&result, "ABTC") > 0);
    let hit = result
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(hit.source.unwrap().line, 9);
    let excerpt =
        mant_query::select_excerpt(&content, &[ContentSelector::path(hit.outline.path())]).unwrap();
    assert!(mant_render::render_excerpt_text(&excerpt).contains(BODY));
    assert_eq!(explain(&content, "ATC").counts.direct_entry.total, 0);
}

#[test]
fn argument_text_fragments_keep_their_native_parameter_scope() {
    // CVS mdoc_macro.c::in_line allocates TEXT children under one Ar.
    // The escaped closing delimiters below stay inside that element.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (argument, reading) in [
            (r#"Ar "[source target]""#, "[source target]"),
            (r"Ar [source target\&]", "[source target]"),
            (r"Ar range(low high\&)", "range(low high)"),
            (r"Ar map[(low high)\&]", "map[(low high)]"),
            (r"Ar path[in Ns Ar dex\&]", "path[index]"),
        ] {
            assert_program_group(
                section,
                &format!("Nm alpha {argument}"),
                &format!("alpha {reading}"),
                &["alpha"],
                &[],
            );
            assert_program_group(
                section,
                &format!("Nm alpha Op {argument}"),
                &format!("alpha [{reading}]"),
                &["alpha"],
                &[],
            );
            assert_program_group(
                section,
                &format!("Nm alpha Cm dump Oo {argument} | Nm beta Oc"),
                &format!("alpha dump [{reading} | beta]"),
                &["alpha dump"],
                &["beta"],
            );
            assert_program_group(
                section,
                &format!("Nm alpha {argument} | Nm beta"),
                &format!("alpha {reading} | beta"),
                &[],
                &["alpha", "beta"],
            );
        }
    }
}

#[test]
fn accepted_parameter_edges_keep_comma_aliases_and_reject_top_level_pipes() {
    // CVS in_line() closes Ar at bare delimiters; attached edge glyphs
    // remain in their original TEXT, even when Ns joins the next call.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (head, form, names, rejected) in [
            (
                "Nm alpha Ar path[ Ns Ar index ]",
                "alpha path[index]",
                &["alpha"][..],
                &["index"][..],
            ),
            (
                "Nm alpha [ Ar source] target | Nm beta Ar path[ Ns Ar index ]",
                "alpha [source] target | beta path[index]",
                &[][..],
                &["alpha", "beta"][..],
            ),
            (
                "Nm gamma , Nm alpha Ar path[ Ns Em index ]",
                "gamma, alpha path[index]",
                &["gamma", "alpha"][..],
                &["index"][..],
            ),
            (
                "Nm alpha Ar path[ Ns No index ] , Nm gamma",
                "alpha path[index], gamma",
                &["alpha", "gamma"][..],
                &["index"][..],
            ),
            (
                "Nm alpha Cm dump Oo Ar path[ Ns Ar index ] | Nm beta Oc",
                "alpha dump [path[index] | beta]",
                &["alpha dump"][..],
                &["beta"][..],
            ),
            (
                "Nm alpha [ Ar source] target | Nm beta Ar path[ Ns Ar index ] , Nm gamma",
                "alpha [source] target | beta path[index], gamma",
                &["gamma"][..],
                &["alpha", "beta"][..],
            ),
        ] {
            assert_program_group(section, head, form, names, rejected);
            assert_program_group(section, &format!("Xo\n.{head}\n.Xc"), form, names, rejected);
        }
    }
}

#[test]
fn nested_parameter_delimiters_leave_outer_aliases_addressable() {
    // CVS in_line() may split Ar at quote and macro boundaries. Its
    // native enclosure BODY owns the inner bracket and the detached close.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (start, end, left, right) in [
            ("", "", "", ""),
            (" Oo", " Oc", "[", "]"),
            (" Po", " Pc", "(", ")"),
            (" Bro", " Brc", "{", "}"),
            (" Oo Po", " Pc Oc", "[(", ")]"),
        ] {
            for (arg, reading) in [
                ("Ar path[ Ns Em index ]", "path[index]"),
                ("Ar path[ Ns No index ]", "path[index]"),
                ("Ar path[ index ]", "path[ index]"),
                (r#"Ar "path[ index" ]"#, "path[ index]"),
            ] {
                for (role, alias) in [
                    ("Nm", "gamma"),
                    ("Va", "var_b"),
                    ("Er", "EINVAL"),
                    ("Dv", "CONST_B"),
                ] {
                    let invocation = format!("Nm alpha{start} {arg}{end}");
                    let form = format!("alpha {left}{reading}{right}");
                    for (head, form, names) in [
                        (
                            format!("{invocation} , {role} {alias}"),
                            format!("{form}, {alias}"),
                            ["alpha", alias],
                        ),
                        (
                            format!("{role} {alias} , {invocation}"),
                            format!("{alias}, {form}"),
                            [alias, "alpha"],
                        ),
                    ] {
                        assert_program_group(section, &head, &form, &names, &["index"]);
                        assert_program_group(
                            section,
                            &format!("Xo\n.{head}\n.Xc"),
                            &form,
                            &names,
                            &["index"],
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn nested_commands_do_not_turn_an_outer_pipe_into_an_alias_separator() {
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (head, form, names) in [
            (
                "Nm alpha Ar file | Nm beta Oo [ Ar sub] target Oc , Nm gamma",
                "alpha file | beta [[sub] target], gamma",
                &["gamma"][..],
            ),
            (
                "Nm alpha Oo Ar path[ Ns Ar index ] Oc | Nm beta Ar x",
                "alpha [path[index]] | beta x",
                &[][..],
            ),
            (
                "Nm alpha Oo Po Ar path[ Ns Em index ] Pc Oc | Nm beta Ar x , Nm gamma",
                "alpha [(path[index])] | beta x, gamma",
                &["gamma"][..],
            ),
            (
                "Nm alpha Oo Po Ar path[ Ns Em index ] Pc | Nm beta Oc , Nm gamma",
                "alpha [(path[index]) | beta], gamma",
                &["alpha", "gamma"][..],
            ),
        ] {
            let rejected = if names.contains(&"alpha") {
                &["beta"][..]
            } else {
                &["alpha", "beta"][..]
            };
            assert_program_group(section, head, form, names, rejected);
            assert_program_group(section, &format!("Xo\n.{head}\n.Xc"), form, names, rejected);
        }
    }
}

#[test]
fn a_detached_parameter_closer_can_finish_the_same_display_word() {
    for section in ["DESCRIPTION", "COMMANDS"] {
        for head in [
            "Nm alpha Ar path[index ]",
            "Xo\n.Nm alpha Ar path[index ]\n.Xc",
        ] {
            assert_program_group(section, head, "alpha path[index]", &["alpha"], &["index"]);
        }
    }
}

#[test]
fn scopes_pair_block_prefix_delimiters_and_detached_closers_before_aliases() {
    // CVS blk_part_exp puts leading punctuation before HEAD, but an
    // authored close can execute on either side of the native BODY post.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (head, form) in [
            (
                "Nm alpha Oo ( Ar file Oc ) , Nm gamma",
                "alpha ([file]), gamma",
            ),
            (
                "Nm alpha ( Oo Ar file Oc ) , Nm gamma",
                "alpha ([file]), gamma",
            ),
            (
                "Nm alpha Oo ( Ar file ) Oc , Nm gamma",
                "alpha ([file)], gamma",
            ),
            (
                "Nm alpha Ar path[[index ] ] , Nm gamma",
                "alpha path[[index]], gamma",
            ),
            (
                "Nm alpha Ar path[[index]] , Nm gamma",
                "alpha path[[index]], gamma",
            ),
        ] {
            for spelling in [head.to_owned(), format!("Xo\n.{head}\n.Xc")] {
                let argument = if form.contains("file") {
                    "file"
                } else {
                    "index"
                };
                check(
                    &source(section, &spelling),
                    form,
                    &["alpha", "gamma"],
                    &[argument],
                    BODY,
                    9,
                );
            }
        }
        for head in [
            "Nm alpha Oo ( Ar file Oc ) | Nm beta , Nm gamma",
            "Xo\n.Nm alpha Oo ( Ar file Oc ) | Nm beta , Nm gamma\n.Xc",
        ] {
            assert_program_group(
                section,
                head,
                "alpha ([file]) | beta, gamma",
                &["gamma"],
                &["alpha", "beta", "file"],
            );
        }
    }
}

#[test]
fn native_environment_aliases_ignore_arguments_in_another_scope() {
    // The original BODY retains its own Ar/Em text. An adjacent Ev
    // outside the BODY has independent evidence on either comma side.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (head, form) in [
            (
                "Ev ENV_A Oo Ar path[ Ns Em index ] Oc , Ev ENV_B",
                "ENV_A [path[index]], ENV_B",
            ),
            (
                "Ev ENV_B , Ev ENV_A Oo Ar path[ Ns Em index ] Oc",
                "ENV_B, ENV_A [path[index]]",
            ),
            (
                "Ev ENV_A Po Ar path[ Ns Em index ] Pc , Ev ENV_B",
                "ENV_A (path[index]), ENV_B",
            ),
            (
                "Ev ENV_A Oo Ar path[ Ns Ar index ] Oc , Ev ENV_B",
                "ENV_A [path[index]], ENV_B",
            ),
        ] {
            assert_program_group(section, head, form, &["ENV_B"], &["index"]);
            assert_program_group(
                section,
                &format!("Xo\n.{head}\n.Xc"),
                form,
                &["ENV_B"],
                &["index"],
            );
        }
    }
}

#[test]
fn quoted_arguments_and_reopened_flags_keep_their_invocation_owner() {
    // mdoc_macro.c::in_line reopens Fl after its comma. The reopened
    // operand looks identical to a second authored Fl in the terminal.
    for section in ["COMMANDS", "DESCRIPTION"] {
        for (head, form, names, non_names) in [
            (
                "Nm tool Qo reason Qc Qo ... Qc",
                "tool \"reason\" \"...\"",
                &["tool"][..],
                &["reason"][..],
            ),
            (
                "Nm tool Qq filename",
                "tool \"filename\"",
                &["tool"],
                &["filename"],
            ),
            (
                "Nm tool Fl V , -version",
                "tool -V, --version",
                &["tool"],
                &["--version"],
            ),
            (
                "Nm tool Fl V , Fl -version",
                "tool -V, --version",
                &["tool", "--version"],
                &[],
            ),
        ] {
            assert_program_group(section, head, form, names, non_names);
        }
    }
}

fn assert_program_group(section: &str, head: &str, form: &str, names: &[&str], non_names: &[&str]) {
    let input = source(section, head);
    let content = load(&input);
    let items = super::definitions(content.document.as_ref().unwrap());
    let [item] = items.as_slice() else {
        panic!("{input}")
    };
    check_head(item, form, names);
    for name in names {
        super::real_invocations::direct_lines(&content, name, &[9]);
    }
    for rejected in non_names {
        super::real_invocations::direct_lines(&content, rejected, &[]);
    }
}

#[test]
fn execution_qualifies_compound_programs_and_nested_arguments_in_scope() {
    // CVS termp_nm_pre prints both words of one explicit Nm name, and
    // termp_quote_pre/post delimit the matching optional and quoted BODYs.
    // Separate Nm invocations across a pipe do not describe one declaration.
    for section in ["COMMANDS", "DESCRIPTION"] {
        let command_section = section == "COMMANDS";
        for (head, form, names, rejected) in [
            (
                "Nm zpool iostat",
                "zpool iostat",
                if command_section {
                    &["zpool iostat"][..]
                } else {
                    &[]
                },
                &["zpool"][..],
            ),
            (
                "Nm zpool Cm iostat",
                "zpool iostat",
                &["zpool iostat"],
                &["zpool"],
            ),
            (
                "Nm alpha Oo Qo filename Qc Oc",
                "alpha [\"filename\"]",
                &["alpha"],
                &["filename"],
            ),
            (
                "Nm alpha Op Qq filename",
                "alpha [\"filename\"]",
                &["alpha"],
                &["filename"],
            ),
            (
                "Nm alpha Op No filename",
                "alpha [filename]",
                &[],
                &["alpha", "filename"],
            ),
            (
                "Nm alpha Cm dump | Nm beta Cm read",
                "alpha dump | beta read",
                &[],
                &["alpha dump", "beta read"],
            ),
            (
                "Nm alpha Cm dump Ar arg | Nm beta Ar other",
                "alpha dump arg | beta other",
                &[],
                &["alpha dump", "beta"],
            ),
            (
                "Nm alpha Cm dump , Nm beta Cm read",
                "alpha dump, beta read",
                &["alpha dump", "beta read"],
                &["alpha", "beta"],
            ),
            (
                "Nm alpha Cm dump Ar a | Ar b",
                "alpha dump a | b",
                &["alpha dump"],
                &["a", "b"],
            ),
        ] {
            assert_program_group(section, head, form, names, rejected);
        }
    }
}

#[test]
fn literal_suffixes_cannot_join_another_flags_executed_dash() {
    // termp_fl_pre emits its own dash. The intervening No in "-qx-a" does
    // not transfer that second Fl's glyphs to the name of the first one.
    for section in ["COMMANDS", "DESCRIPTION"] {
        for (head, spelling, positive) in [
            ("Fl q Ns No x Ns Fl a", "-qx-a", false),
            ("Fl q Ns No x Ns Fl Fl beta", "-qx--beta", false),
            ("Fl q Ns No x Ns No y", "-qxy", true),
        ] {
            for suffix in ["", " , Fl z"] {
                let content = load(&source(section, &format!("{head}{suffix}")));
                super::real_invocations::direct_lines(
                    &content,
                    spelling,
                    if positive { &[9] } else { &[] },
                );
                super::real_invocations::direct_lines(
                    &content,
                    "-z",
                    if suffix.is_empty() { &[] } else { &[9] },
                );
            }
        }
    }
}
