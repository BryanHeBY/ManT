//! Separate native macro operands can still belong to the same invocation.
use super::{
    definitions,
    support::{check_head, explain, explanation_ranges, load},
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
