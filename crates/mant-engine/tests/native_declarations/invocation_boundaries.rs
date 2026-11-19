//! Separate native macro operands can still belong to the same invocation.
use super::{
    definitions,
    support::{check_head, explain, explanation_ranges, json_roundtrip, load},
};
use mant_ir::{DefinitionItem, EntryKind, EntryOwner, ParameterKind, ResolvedContent};
use mant_protocol::{
    ContentSelector, EvidenceClass, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n";
const OWNER_LINE: u32 = 9;
const OPTION: EntryKind = EntryKind::Parameter {
    parameter_kind: ParameterKind::Option,
};

#[test]
fn generated_function_punctuation_only_indexes_the_surviving_name() {
    // In mdoc_term.c::termp_fn_pre() the generated "(" is a term_word().
    // On both fixed devices it covers M after a one-slot retreat and
    // leaves one reading copy of M after the two-slot retreat.
    for (distance, form, direct) in [(1, "(arg)", 0), (2, "M(arg)", 1)] {
        let input = format!(
            "{HEADER}.Sh DESCRIPTION\n.Bl -tag -width 12n\n.It Fn \"M\\h'-{distance}n'\" arg\nOwned description.\n.El\n"
        );
        let content = json_roundtrip::json_string_roundtrip(&load(&input));
        let text = mant_render::render_query_text(&content);
        let has_label_and_body = |line: &str| {
            line.trim()
                .trim_start_matches("• ")
                .strip_prefix(form)
                .is_some_and(|body| body.trim() == "Owned description.")
        };
        assert!(text.lines().any(has_label_and_body), "{input}\n{text}");
        assert_eq!(
            explain(&content, "M").counts.direct_entry.total,
            direct,
            "{input}"
        );
        let markdown = mant_codec::encode::render_markdown(&content);
        let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let text = mant_render::render_query_text(&restored);
        assert!(text.lines().any(has_label_and_body), "{markdown}\n{text}");
    }
}

fn page(section: &str, head: &str, body: &str) -> ResolvedContent {
    load(&format!(
        "{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\n{body}\n.El\n"
    ))
}

fn owner(content: &ResolvedContent) -> &DefinitionItem {
    let items = definitions(content.document.as_ref().unwrap());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source.unwrap().line, OWNER_LINE);
    items[0]
}

fn direct(content: &ResolvedContent, name: &str, body: &str) {
    let result = explain(content, name);
    assert_eq!(result.counts.direct_entry.total, 1, "{name}");
    assert!(
        explanation_ranges::assert_ranges(&result, name) > 0,
        "{name}"
    );
    let evidence = result
        .evidence
        .iter()
        .find(|entry| entry.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(evidence.source, owner(content).source);
    let excerpt =
        mant_query::select_excerpt(content, &[ContentSelector::path(evidence.outline.path())])
            .unwrap();
    assert!(mant_render::render_excerpt_text(&excerpt).contains(body));
}

fn visible_argument(content: &ResolvedContent, name: &str) {
    assert_eq!(
        explain(content, name).counts.direct_entry.total,
        0,
        "{name}"
    );
    let result = mant_query::search_query(
        content,
        &SearchQuery {
            pattern: name.into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: true,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    assert!(result.content_complete);
    assert!(result.total > 0, "{name}");
}

#[test]
fn a_separate_symbolic_operand_keeps_its_text_without_extending_the_command_name() {
    // The pinned tree gives Cm/Ic and Sy separate elements, even though
    // mdoc_term.c::termp_bold_pre styles both and the complete form is visible.
    for section in ["DESCRIPTION", "OPTIONS", "ENVIRONMENT", "COMMANDS"] {
        for (head, form, name, argument) in [
            ("Cm run Sy FILE", "run FILE", "run", "FILE"),
            ("Ic run Sy file", "run file", "run", "file"),
            (
                "Cm run Cm check Sy file",
                "run check file",
                "run check",
                "file",
            ),
            (
                "Cm run Nm check Sy file",
                "run check file",
                "run check",
                "file",
            ),
        ] {
            let content = page(section, head, "BODY");
            check_head(owner(&content), form, &[name]);
            direct(&content, name, "BODY");
            direct(&content, form, "BODY");
            visible_argument(&content, argument);
        }
        // Ns executes a joined word. A source scope change cannot by itself
        // make the shorter Cm spelling an independent visible name.
        let joined = page(section, "Cm run Ns Sy FILE", "BODY");
        check_head(owner(&joined), "runFILE", &[]);
        assert_eq!(explain(&joined, "run").counts.direct_entry.total, 0);
    }

    let styled = page("COMMANDS", "Sy run FILE", "BODY");
    check_head(owner(&styled), "run FILE", &["run FILE"]);
    let assignment = page("ENVIRONMENT", "Cm TOOL=FILE", "BODY");
    check_head(owner(&assignment), "TOOL=FILE", &["TOOL"]);
    assert_eq!(
        EntryOwner::Definition(owner(&assignment))
            .facts()
            .unwrap()
            .kind,
        EntryKind::EnvironmentVariable
    );
}

#[test]
fn punctuation_inside_a_symbolic_operand_does_not_open_another_name() {
    // mdoc_macro.c::in_line retains quoted punctuation in the Sy text
    // node and closes that scope before the separate comma.
    for section in ["DESCRIPTION", "OPTIONS", "ENVIRONMENT", "COMMANDS"] {
        for (head, form, names) in [
            (r#"Cm run Sy "file,other""#, "run file,other", &["run"][..]),
            (r#"Cm run Sy "file|other""#, "run file|other", &["run"][..]),
            (
                r#"Cm run Sy "file,other" , Cm extra"#,
                "run file,other, extra",
                &["run", "extra"][..],
            ),
            (
                r#"Ic run Sy "file|other" , Cm extra"#,
                "run file|other, extra",
                &["run", "extra"][..],
            ),
        ] {
            let content = page(section, head, "BODY");
            check_head(owner(&content), form, names);
            for name in names {
                direct(&content, name, "BODY");
            }
            for operand in ["file", "other"] {
                visible_argument(&content, operand);
            }
        }
    }
}

#[test]
fn option_arguments_remain_readable_without_owning_names() {
    // CVS mdoc_macro.c::in_line closes/reopens Cm or Ic at the middle
    // delimiter. The extra node does not signal a new declaration.
    for section in ["DESCRIPTION", "OPTIONS"] {
        for marker in ["Cm", "Ic"] {
            for (delimiter, spaced, joined) in [
                ("|", "red | blue", "red|blue"),
                (",", "red, blue", "red,blue"),
            ] {
                for (tail, form) in [
                    (delimiter.to_owned(), spaced),
                    (format!("{delimiter} {marker}"), spaced),
                    (format!("Ns {delimiter} Ns {marker}"), joined),
                ] {
                    let head = format!("Fl M {marker} red {tail} blue");
                    let body = "The -M option accepts one value, red or blue.";
                    let content = page(section, &head, body);
                    let item = owner(&content);
                    check_head(item, &format!("-M {form}"), &["-M"]);
                    assert_eq!(EntryOwner::Definition(item).facts().unwrap().kind, OPTION);
                    direct(&content, "-M", body);
                    for value in ["red", "blue"] {
                        visible_argument(&content, value);
                    }
                }
            }
        }
    }
}

#[test]
fn mdoc_operand_pipe_is_not_a_definition_separator() {
    // in_line() exits the Ar element at the bare middle delimiter. The
    // right-hand style, wrapper, and typed reference all preserve its use.
    let body = "Read a record from the output of the command denoted by cmd.";
    for command in ["Ic", "Cm", "Nm", "Xr", "Sy", "Ql", "Op Ic"] {
        let version = if command == "Xr" { " 1" } else { "" };
        let invocation = format!(".Ar cmd | {command} getline{version} Op Va var");
        for (lines, names) in [
            (invocation.clone(), &[][..]),
            (format!("{invocation}\n.No \\&,\n.Fn extra"), &["extra"][..]),
            (format!(".Fn extra\n.No \\&,\n{invocation}"), &["extra"][..]),
        ] {
            let head = format!("Xo\n{lines}\n.Xc");
            let content = page("COMMANDS", &head, body);
            let item = owner(&content);
            assert_eq!(
                EntryOwner::Definition(item).validated_names().unwrap(),
                names
            );
            for argument in ["cmd", "var"] {
                visible_argument(&content, argument);
            }
            for name in names {
                direct(&content, name, body);
            }
        }
    }
}

#[test]
fn a_standalone_flag_keeps_its_selectable_owner_below_a_macro_signature() {
    // mdoc_macro.c::in_line reopens Fl inside the Ic signature.
    // The separate It below the signature is the only flag declaration.
    let content = page(
        "OPTIONS",
        r"Ic \&An Fl split | nosplit | Ar first_name ... last_name",
        "Author name.\n.Bl -tag -width Ds\n.It Fl nosplit\nKeep author names together.\n.El",
    );
    let items = definitions(content.document.as_ref().unwrap());
    assert_eq!(items.len(), 2);
    check_head(
        items[0],
        "An -split | -nosplit | first_name ... last_name",
        &["An"],
    );
    check_head(items[1], "-nosplit", &["-nosplit"]);
    for (index, key, line, marker) in [
        (0, "An", 9, "Author name."),
        (1, "-nosplit", 12, "Keep author names together."),
    ] {
        let explanation = explain(&content, key);
        assert_eq!(explanation.counts.direct_entry.total, 1);
        assert!(explanation_ranges::assert_ranges(&explanation, key) > 0);
        let hit = explanation
            .evidence
            .iter()
            .find(|entry| entry.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(hit.source, items[index].source);
        assert_eq!(hit.source.unwrap().line, line);
        let excerpt =
            mant_query::select_excerpt(&content, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        assert!(mant_render::render_excerpt_text(&excerpt).contains(marker));
    }
}

#[test]
fn independently_proved_aliases_can_share_one_bare_pipe() {
    for (section, head, form, names) in [
        ("OPTIONS", "Fl a | Fl b", "-a | -b", &["-a", "-b"][..]),
        (
            "OPTIONS",
            "Fl a | Ic allexport",
            "-a | allexport",
            &["-a", "allexport"],
        ),
        (
            "DESCRIPTION",
            "Ar left | Ar right",
            "left | right",
            &["left", "right"],
        ),
    ] {
        let content = page(section, head, "OWNERBODY.");
        check_head(owner(&content), form, names);
        for name in names {
            direct(&content, name, "OWNERBODY.");
        }
    }
}

#[test]
fn configuration_and_command_arguments_keep_their_leading_owner() {
    // mdoc_macro.c::in_line closes each displayed argument at its own
    // delimiter. In these real pf.conf(5) and zfs(8) forms the leading
    // keyword/assignment, not the subsequent Cm styles, owns the BODY.
    for (head, names, visible, arguments) in [
        (
            "Cm flags Ar a Ns / Ns Ar b | Cm any",
            &["flags"][..],
            "flags a/b | any",
            &["a", "b", "any"][..],
        ),
        (
            "Sy compression Ns = Ns Cm on | off | lzjb | gzip | gzip- Ns Ar N | Cm zle | Cm lz4",
            &["compression"],
            "compression=on | off | lzjb | gzip | gzip-N | zle | lz4",
            &["on", "off", "lzjb", "gzip", "zle", "lz4"],
        ),
        (
            "Sy mountpoint Ns = Ns Ar path | Cm none | legacy",
            &["mountpoint"],
            "mountpoint=path | none | legacy",
            &["path", "none", "legacy"],
        ),
    ] {
        let content = page("DESCRIPTION", head, "OWNERBODY.");
        check_head(owner(&content), visible, names);
        direct(&content, names[0], "OWNERBODY.");
        for argument in arguments {
            visible_argument(&content, argument);
        }
    }
}

#[test]
fn a_display_does_not_split_a_proved_option_argument_list() {
    // The pinned It HEAD contains both options' native operands and the
    // intervening tbl receipt. Only the final comma starts a sibling.
    for section in ["OPTIONS", "DESCRIPTION"] {
        for separator in [r".No \&|", r".No \&,"] {
            for position in 0..=4 {
                let mut heads = vec![
                    ".Fl M",
                    ".Cm red",
                    separator,
                    ".Cm blue",
                    r".No \&,",
                    ".Fn extra",
                ];
                heads.insert(position, ".TS\nl.\nTABLECELL\n.TE");
                let head = format!("Xo\n{}\n.Xc", heads.join("\n"));
                let content = page(section, &head, "OWNERBODY.");
                let item = owner(&content);
                assert_eq!(
                    EntryOwner::Definition(item).validated_names().unwrap(),
                    ["-M", "extra"],
                    "{head}"
                );
                for name in ["-M", "extra"] {
                    direct(&content, name, "OWNERBODY.");
                }
                for value in ["red", "blue", "TABLECELL"] {
                    visible_argument(&content, value);
                }
            }
        }
    }
}

#[test]
fn independent_names_and_invocation_values_use_distinct_budget() {
    for count in [255, 256, 257] {
        let values = (0..count)
            .map(|index| format!("arg{index}"))
            .collect::<Vec<_>>();
        let head = format!("Fl M Cm {}", values.join(" | "));
        let content = page("OPTIONS", &head, "OWNERBODY.");
        check_budget(&content, "-M", false);
        assert_eq!(
            EntryOwner::Definition(owner(&content))
                .validated_names()
                .unwrap(),
            ["-M"]
        );
        visible_argument(&content, values.last().unwrap());

        let options = values[..count - 1]
            .iter()
            .map(|value| format!(".Fl Fl {value}"));
        let groups = std::iter::once(".Er PIVOTERR".to_owned())
            .chain(options)
            .collect::<Vec<_>>();
        let head = format!("Xo\n{}\n.Xc", groups.join("\n.No \\&,\n"));
        let content = page("OPTIONS", &head, "OWNERBODY.");
        check_budget(&content, "--arg0", count > 256);
    }
}

fn check_budget(content: &ResolvedContent, key: &str, limited: bool) {
    let explanation = explain(content, key);
    assert_eq!(explanation.counts.direct_entry.total, u32::from(!limited));
    assert_ne!(explanation.semantics_complete, limited);
    assert_eq!(
        explanation
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_deref() == Some("manual.semantic-entry.name-limit")
            })
            .count(),
        usize::from(limited)
    );
}
