use std::collections::{HashMap, HashSet};

use mant_ir::{
    Block, DefinitionItem, EntryFacts, EntryKind, Heading, Inline, LayoutHint, NameCase, Section,
};

use super::{environment_variable_alias, option_prefix};
use crate::test_content as fixture;

#[cfg(feature = "roff")]
fn native_flow_entries(input: &[u8], label: &str) -> Vec<(Vec<String>, EntryKind, NameCase)> {
    use mant_ir::visit::{self, Visit};

    struct Collector(Vec<(Vec<String>, EntryKind, NameCase)>);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
            if let Some(entry) = &item.entry {
                self.0.push((entry.names.clone(), entry.kind, entry.case));
            }
            visit::walk_definition_item(self, item);
        }
    }

    let document = crate::mandoc::parse_plain_manual(std::path::Path::new(label), input)
        .expect("pinned-CVS-checked roff parses");
    let mut collector = Collector(Vec::new());
    collector.visit_document(&document);
    collector.0
}

#[cfg(feature = "roff")]
#[test]
fn weak_pp_rs_variable_and_configuration_heads_need_complete_local_syntax() {
    // This exact SSH_CONFIG page ran pinned CVS -Tutf8 -Owidth=78 first.
    // man_term.c::pre_PP/pre_RS establish the visible paragraph and direct
    // indentation, not a semantic variable/configuration category.
    let input = b".TH SSH_CONFIG 5\n.SH VARIABLES\n.PP\n.B foo=bar\n.RS 4\nFoo assignment.\n.RE\n.PP\n.B foo <S>\n.RS 4\nFoo placeholder.\n.RE\n.PP\n.B foo\n.RS 4\nBare variable prose.\n.RE\n.PP\n.B foo=\n.RS 4\nIncomplete variable prose.\n.RE\n.PP\n.B <K><S>\n.RS 4\nPlaceholder prose.\n.RE\n.SH DESCRIPTION\n.PP\n.B BatchMode=yes\n.RS 4\nRoot undotted configuration assignment.\n.RE\n.PP\n.B BatchMode\n.RS 4\nBare root configuration prose.\n.RE\n.PP\n.B core.editor=vim\n.RS 4\nRoot configuration assignment.\n.RE\n.PP\n.B core.editor\n.RS 4\nBare root configuration prose.\n.RE\n.SH CONFIGURATION\n.PP\n.B local.key\n.RS 4\nLocal configuration key.\n.RE\n";
    let entries = native_flow_entries(input, "ssh_config.5");
    assert_eq!(entries.len(), 5, "{entries:?}");
    assert_eq!(
        entries
            .iter()
            .filter(|(names, kind, _)| names == &["foo"] && *kind == EntryKind::Variable)
            .count(),
        2
    );
    for name in ["BatchMode", "core.editor", "local.key"] {
        let (_, kind, case) = native_entry(&entries, name);
        assert_eq!(*kind, EntryKind::ConfigurationKey);
        assert_eq!(*case, NameCase::Sensitive);
    }
}

#[cfg(feature = "roff")]
fn native_entry<'a>(
    entries: &'a [(Vec<String>, EntryKind, NameCase)],
    name: &str,
) -> &'a (Vec<String>, EntryKind, NameCase) {
    entries
        .iter()
        .find(|(names, _, _)| names.iter().any(|found| found == name))
        .unwrap_or_else(|| panic!("missing entry {name}: {entries:?}"))
}

#[cfg(feature = "roff")]
#[test]
fn native_literal_role_follows_local_declaration_context() {
    // This is exactly EN00 roles.1, rendered with pinned CVS -Tutf8 first.
    // mdoc_macro.c::blk_full retains each It HEAD; mdoc_term.c::termp_it_pre
    // prints Ic/Cm in bold but does not assign a ManT Command/Config type.
    let input = b".Dd September 26, 2026\n.Dt ENTRY-ROLES 1\n.Os\n.Sh NAME\n.Nm entry-roles\n.Nd declaration recognition probe\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Ev DEMO_HOME\nHome directory for the program.\n.It Va counter\nA variable, not an environment variable.\n.It Ic activity-action\nA named setting without an explicit command context.\n.It Dv MODE_FAST\nA symbolic constant.\n.El\n.Sh CONFIGURATION\n.Bl -tag -width Ds\n.It Cm BatchMode\nSet the batch processing behavior.\n.El\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Ic attach-session Ar target\nAttach to a session.\n.El\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev TMPDIR , Ev TEMP , Ev TMP\nVariables checked in this order.\n.El\n";
    let entries = native_flow_entries(input, "entry-roles.1");
    assert_eq!(
        native_entry(&entries, "DEMO_HOME").1,
        EntryKind::EnvironmentVariable
    );
    assert_eq!(native_entry(&entries, "counter").1, EntryKind::Variable);
    assert_eq!(native_entry(&entries, "activity-action").1, EntryKind::Term);
    assert_eq!(native_entry(&entries, "MODE_FAST").1, EntryKind::Term);
    assert_eq!(
        native_entry(&entries, "BatchMode").1,
        EntryKind::ConfigurationKey
    );
    assert_eq!(native_entry(&entries, "BatchMode").2, NameCase::Sensitive);
    assert_eq!(
        native_entry(&entries, "attach-session").1,
        EntryKind::Command
    );
    assert_eq!(
        native_entry(&entries, "TMPDIR").0,
        ["TMPDIR", "TEMP", "TMP"]
    );
}

#[cfg(feature = "roff")]
#[test]
fn canonical_config_root_survives_description_but_not_reference_sections() {
    // This exact SSH_CONFIG/It sample ran through pinned CVS -Tutf8 first.
    // mdoc_term.c::termp_it_pre supplies the physical list ownership;
    // document-title hints and semantic case policy are ManT-only.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm BatchMode\nEnable batch behavior.\n.It Cm Verbose\nKeep a second key.\n.El\n.Sh SEE ALSO\n.Bl -tag -width Ds\n.It Ic unrelated-setting\nAn unrelated term.\n.El\n";
    let entries = native_flow_entries(input, "renamed-input.5");
    for name in ["BatchMode", "Verbose"] {
        let (_, kind, case) = native_entry(&entries, name);
        assert_eq!(*kind, EntryKind::ConfigurationKey, "{name}");
        assert_eq!(*case, NameCase::Sensitive, "{name}");
    }
    assert_eq!(
        native_entry(&entries, "unrelated-setting").1,
        EntryKind::Term
    );
}

#[cfg(feature = "roff")]
#[test]
fn native_ar_argument_ends_a_complete_configuration_key() {
    // Exact input rendered with pinned CVS -Tutf8 before this assertion.
    // mdoc_macro.c::blk_full retains distinct Cm/Ar instances in the It
    // HEAD; mdoc_term.c::termp_bold_pre/termp_under_pre execute their fonts.
    // Parse-local Ar boundaries, not merely an underline, license the suffix.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm AddressFamily Ar address_family\nSelect address families.\n.El\n";
    let entries = native_flow_entries(input, "renamed-input.5");
    let (names, kind, case) = native_entry(&entries, "AddressFamily");
    assert_eq!(names, &["AddressFamily"]);
    assert_eq!(*kind, EntryKind::ConfigurationKey);
    assert_eq!(*case, NameCase::Sensitive);
    assert!(
        !entries
            .iter()
            .any(|(names, _, _)| names.iter().any(|name| name == "address_family"))
    );
}

#[cfg(feature = "roff")]
#[test]
fn native_ar_placeholder_is_not_forced_into_value_name_grammar() {
    // Exact HostKeyAlgorithms/Ar algorithm[,algorithm...] input ran pinned
    // CVS -Tutf8 first. mdoc_term.c prints the bracketed Ar operand in the
    // same It HEAD; it is an argument boundary, not an extra Value entry.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm HostKeyAlgorithms Ar algorithm[,algorithm...]\nSelect algorithms.\n.El\n";
    let entries = native_flow_entries(input, "different-name.5");
    assert_eq!(
        native_entry(&entries, "HostKeyAlgorithms").1,
        EntryKind::ConfigurationKey
    );
    assert!(
        entries
            .iter()
            .all(|(names, _, _)| { !names.iter().any(|name| name.contains("algorithm")) })
    );
}

#[cfg(feature = "roff")]
#[test]
fn incomplete_assignment_and_nested_argument_do_not_become_config_keys() {
    // Each exact input ran through pinned CVS -Tutf8 first. It/Bl preserve
    // native owner and nesting; neither the malformed bracket nor TP/It
    // indentation is a configuration language role.
    let incomplete = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh CONFIGURATION\n.Bl -tag\n.It Cm color=[yes|no\nIncomplete literal.\n.El\n";
    let entries = native_flow_entries(incomplete, "incomplete.1");
    assert!(
        entries
            .iter()
            .all(|(_, kind, _)| *kind != EntryKind::ConfigurationKey)
    );

    let nested = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl mode\nMode.\n.Bl -tag\n.It fast=1\nNested item.\n.El\n.El\n";
    let entries = native_flow_entries(nested, "nested.1");
    assert!(
        entries
            .iter()
            .all(|(_, kind, _)| *kind != EntryKind::ConfigurationKey)
    );
    assert!(matches!(
        native_entry(&entries, "-mode").1,
        EntryKind::Parameter { .. }
    ));
}

#[cfg(feature = "roff")]
#[test]
fn native_components_bind_multiple_config_and_variable_names_once() {
    // This exact combined Cm/Ar/Em/Va page ran pinned CVS -Tutf8 first.
    // mdoc_macro.c::blk_full keeps each macro instance in one It HEAD;
    // term_word() executes final glyphs and spacing. The instance markers
    // supply names, while complete visible gaps prevent punctuation alone
    // from manufacturing another declaration.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm Alpha , Cm Beta\nTwo keys.\n.It Cm ProxyCommand Ar command Ar argument\nOne key with two parameters.\n.It Cm Key Em prose\nThis is not a complete key declaration.\n.El\n.Sh VARIABLES\n.Bl -tag -width Ds\n.It Va alpha , Va beta\nTwo variables.\n.El\n";
    let entries = native_flow_entries(input, "other-input.5");
    assert_eq!(native_entry(&entries, "Alpha").0, ["Alpha", "Beta"]);
    assert_eq!(native_entry(&entries, "Beta").0, ["Alpha", "Beta"]);
    assert_eq!(
        native_entry(&entries, "Alpha").1,
        EntryKind::ConfigurationKey
    );
    assert_eq!(native_entry(&entries, "ProxyCommand").0, ["ProxyCommand"]);
    assert_eq!(
        native_entry(&entries, "ProxyCommand").1,
        EntryKind::ConfigurationKey
    );
    assert!(!entries.iter().any(|(names, _, _)| {
        names
            .iter()
            .any(|name| name == "command" || name == "argument")
    }));
    assert!(entries.iter().all(|(names, kind, _)| {
        !names.iter().any(|name| name == "Key") || *kind != EntryKind::ConfigurationKey
    }));
    assert_eq!(native_entry(&entries, "alpha").0, ["alpha", "beta"]);
    assert_eq!(native_entry(&entries, "beta").1, EntryKind::Variable);
}

#[cfg(feature = "roff")]
#[test]
fn joined_native_instances_do_not_create_two_names_or_an_argument() {
    // Both exact Ns inputs ran pinned CVS -Tutf8 first. mdoc_term.c prints
    // AlphaBeta and BatchMode as joined glyphs; distinct AST macro instances
    // are not independently addressable names without a visible boundary.
    let joined_names = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm Alpha Ns Cm Beta\nOne joined token.\n.El\n";
    let entries = native_flow_entries(joined_names, "joined.5");
    assert!(entries.iter().all(|(names, kind, _)| {
        !names.iter().any(|name| name == "Alpha" || name == "Beta")
            || *kind != EntryKind::ConfigurationKey
    }));
    let joined_argument = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm Batch Ns Ar Mode\nJoined token.\n.El\n";
    let entries = native_flow_entries(joined_argument, "joined.5");
    assert!(entries.iter().all(|(names, kind, _)| {
        !names.iter().any(|name| name == "Batch") || *kind != EntryKind::ConfigurationKey
    }));
}

#[cfg(feature = "roff")]
#[test]
fn native_ar_argument_does_not_feed_option_operand_recognition() {
    // Exact Fl/Ar input ran pinned CVS -Tutf8 first; mdoc_term.c::termp_fl_pre
    // emits -foo, then Ar prints --fake as a distinct underlined argument.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl foo Ar --fake\nDescription.\n.El\n";
    let entries = native_flow_entries(input, "option-argument.1");
    assert!(matches!(
        native_entry(&entries, "-foo").1,
        EntryKind::Parameter { .. }
    ));
    assert!(
        entries
            .iter()
            .all(|(names, _, _)| !names.iter().any(|name| name == "--fake"))
    );
}

#[cfg(feature = "roff")]
#[test]
fn two_native_literal_command_components_bind_two_names() {
    // Exact COMMANDS/It Cm Alpha , Cm Beta input ran pinned CVS -Tutf8 first.
    // mdoc_macro.c::blk_full retains one It HEAD with two Cm instances; their
    // names are independently bound, not an alias relation inferred from ','.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Cm Alpha , Cm Beta\nTwo commands.\n.El\n";
    let entries = native_flow_entries(input, "commands.1");
    assert_eq!(native_entry(&entries, "Alpha").0, ["Alpha", "Beta"]);
    assert_eq!(native_entry(&entries, "Beta").1, EntryKind::Command);
}

#[cfg(feature = "roff")]
#[test]
fn unknown_section_inherits_root_config_but_explicit_barriers_stop_it() {
    // Exact SETTINGS/OPTIONS/EXAMPLES/SEE ALSO page ran pinned CVS -Tutf8
    // first. mdoc_term.c::termp_sh_pre preserves each section boundary; only
    // the finite title-family inheritance is ManT policy.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh SETTINGS\n.Bl -tag\n.It Cm Accept\nAccepted key.\n.El\n.Sh OPTIONS\n.Bl -tag\n.It Cm NotAKey\nOption section.\n.El\n.Sh EXAMPLES\n.Bl -tag\n.It Cm ExampleWord\nExample text.\n.El\n.Sh SEE ALSO\n.Bl -tag\n.It Cm ReferenceWord\nReference text.\n.El\n";
    let entries = native_flow_entries(input, "unrelated-input.5");
    assert_eq!(
        native_entry(&entries, "Accept").1,
        EntryKind::ConfigurationKey
    );
    for name in ["NotAKey", "ExampleWord", "ReferenceWord"] {
        assert_ne!(
            native_entry(&entries, name).1,
            EntryKind::ConfigurationKey,
            "{name}"
        );
    }
}

fn heading(value: &str) -> Heading {
    Heading {
        content: vec![fixture::text(value)],
        source: None,
    }
}

fn option_names(item: &DefinitionItem) -> Vec<String> {
    super::option_names(fixture::content(), item)
}

fn identify_definitions(
    blocks: &mut Vec<Block>,
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
    document_name: Option<&str>,
) -> HashSet<String> {
    let mut content_store = fixture::store();
    super::identify_definitions(
        &mut content_store,
        blocks,
        sections,
        reserved_targets,
        document_name,
    )
}

fn item(value: &str) -> DefinitionItem {
    DefinitionItem {
        source: None,
        entry: None,
        layout: mant_ir::DefinitionLayout {
            inline_term: false,
            spacing_before_lines: None,
            ..Default::default()
        },
        terms: vec![vec![fixture::text(value)]],
        description: Vec::new(),
    }
}

fn strong_item(value: &str) -> DefinitionItem {
    DefinitionItem {
        source: None,
        entry: None,
        layout: mant_ir::DefinitionLayout {
            inline_term: false,
            spacing_before_lines: None,
            ..Default::default()
        },
        terms: vec![vec![Inline::Strong {
            children: vec![fixture::text(value)],
        }]],
        description: Vec::new(),
    }
}

#[test]
fn extracts_aliases_without_argument_placeholders() {
    assert_eq!(
        option_names(&item("-g, --listed-incremental=FILE")),
        ["-g", "--listed-incremental"]
    );
    assert_eq!(option_names(&item("ordinary term")), Vec::<String>::new());
    assert_eq!(option_prefix("-ca.cert"), Some("-ca.cert"));
    assert_eq!(option_prefix("--foo.bar=VALUE"), Some("--foo.bar"));
    assert_eq!(option_prefix("--foo..bar"), None);
}

#[test]
fn slash_aliases_require_complete_option_names_before_the_separator() {
    for form in ["-h/--help", "-h, --help", "-h|--help", "-h/--help FILE"] {
        assert_eq!(option_names(&strong_item(form)), ["-h", "--help"], "{form}");
    }
    for form in [
        "-o /-NUM",
        "-o /tmp/--help",
        "-o=FILE/--help",
        "-o/path/--help",
    ] {
        assert_eq!(option_names(&strong_item(form)), ["-o"], "{form}");
        assert!(super::slash_option_forms(form).is_none(), "{form}");
    }
}

#[test]
fn pattern_and_native_space_heads_keep_only_proved_names() {
    // Exact TP/B and TP/BI inputs ran pinned CVS -Tutf8 first. man_term.c::
    // pre_alternate joins BI operands; chars.c maps \~ to one visible U+00A0.
    // The pattern is syntax, never an alias.
    for form in [
        "-### --long first,--fake,last",
        "-### --long first|--fake|last",
        "-### --long -10,--fake,20",
        "-### --long=FILE",
    ] {
        assert_eq!(option_names(&strong_item(form)), ["--long"], "{form}");
    }
    assert_eq!(
        option_names(&strong_item("-o\u{a0}--output")),
        ["-o", "--output"]
    );
    let term = vec![
        Inline::Strong {
            children: vec![fixture::text("-### --long=")],
        },
        Inline::Emphasis {
            children: vec![fixture::text("FILE")],
        },
    ];
    assert_eq!(
        super::syntax::option_names_from_terms(fixture::content(), &[term]),
        ["--long"]
    );
}

#[test]
fn native_styled_boundaries_do_not_promote_parameter_punctuation() {
    // Pinned CVS man_term.c::pre_alternate directly concatenates BI operands.
    // A later Strong run alone does not prove another operand: term.c::term_word
    // can create the same run via \fB within one italic parameter. The
    // integration test supplies native child witnesses for real BI operands.
    let name = |value| Inline::Strong {
        children: vec![fixture::text(value)],
    };
    let argument = |value| Inline::Emphasis {
        children: vec![fixture::text(value)],
    };
    let terms = [
        (
            vec![
                name("-L"),
                argument("dir,"),
                name("--output="),
                argument("FILE"),
            ],
            vec!["-L"],
        ),
        (vec![name("-L"), argument("dir,--fake,last")], vec!["-L"]),
        (
            vec![
                name("-o"),
                argument("\u{a0}"),
                name("--output "),
                argument("FILE"),
            ],
            vec!["-o", "--output"],
        ),
        (
            vec![
                name("-L"),
                argument("(first,--fake,"),
                name("--all "),
                argument("FILE"),
            ],
            // The final Strong shape alone does not prove a new native BI
            // operand: an inline \fB in the same italic operand looks alike.
            vec!["-L"],
        ),
        (vec![name("-L"), argument("(first,--fake,")], vec!["-L"]),
    ];
    for (term, expected) in terms {
        assert_eq!(
            super::syntax::option_names_from_terms(fixture::content(), &[term]),
            expected
        );
    }
}

#[test]
fn semantic_id_allocation_ignores_a_prefilled_producer_id() {
    let mut option = item("--verbose");
    option.entry = Some(EntryFacts {
        name_bindings: Vec::new(),
        alias_groups: Vec::new(),
        alias_of: None,
        forms: Vec::new(),
        id: "producer-specific-id".into(),
        kind: EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        },
        case: NameCase::Sensitive,
        names: vec!["--verbose".to_owned()],
        value_domain: None,
    });
    let mut sections = vec![Section {
        id: "options".into(),
        fragment_aliases: Vec::new(),
        heading: heading("OPTIONS"),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![option],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    }];

    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

    let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
        panic!("option list");
    };
    assert_eq!(
        items[0].entry.as_ref().expect("identity").id.as_str(),
        "option-verbose"
    );
}

#[test]
fn target_only_definitions_retain_anchors_without_becoming_entries() {
    let target_only = DefinitionItem {
        source: None,
        entry: None,
        layout: mant_ir::DefinitionLayout {
            inline_term: true,
            spacing_before_lines: None,
            ..Default::default()
        },
        terms: vec![vec![fixture::anchor("native-target")]],
        description: Vec::new(),
    };
    let mut sections = vec![Section {
        id: "notes".into(),
        fragment_aliases: Vec::new(),
        heading: heading("NOTES"),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![target_only],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    }];

    let retained = identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

    let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
        panic!("definition list");
    };
    assert!(items[0].entry.is_none());
    assert!(retained.contains("native-target"));
}

#[test]
fn environment_aliases_require_one_complete_semantic_name() {
    for (value, expected) in [
        ("HOME", Some("HOME")),
        ("$Env:Path = C:\\Tools", Some("$Env:Path")),
        (
            "%ProgramFiles(x86)%=C:\\Program Files (x86)",
            Some("%ProgramFiles(x86)%"),
        ),
        ("Unix Bourne shell:", None),
        ("export FOO=bar", None),
        ("LC_ALL=C LANG=en_US", None),
        ("FOO= LANG=en_US", None),
    ] {
        assert_eq!(
            environment_variable_alias(value).as_deref(),
            expected,
            "{value}"
        );
    }
}

#[test]
fn composite_environment_options_use_parameter_semantics() {
    let mut sections = vec![Section {
        id: "environment-options".into(),
        fragment_aliases: Vec::new(),
        heading: heading("ENVIRONMENT OPTIONS"),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![item("Unix Bourne shell:"), item("-q")],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    }];

    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

    let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
        panic!("definition list");
    };
    assert_eq!(items[0].entry.as_ref().expect("term").kind, EntryKind::Term);
    assert_eq!(
        items[1].entry.as_ref().expect("option").kind,
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option
        }
    );
}

#[test]
fn command_discovery_requires_a_structural_or_syntactic_boundary() {
    let definition_list = |items| Block::DefinitionList {
        declaration_groups: Vec::new(),
        items,
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    };
    let mut sections = vec![
        Section {
            id: "commands".into(),
            fragment_aliases: Vec::new(),
            heading: heading("COMMANDS"),
            spacing_before_lines: 0,
            blocks: vec![definition_list(vec![
                strong_item("Send Env"),
                item("Send Buffer"),
                item("bind [-m keymap]"),
                item("set -o"),
                item("0 arguments"),
            ])],
            children: Vec::new(),
            source: None,
        },
        Section {
            id: "variables".into(),
            fragment_aliases: Vec::new(),
            heading: heading("VARIABLES"),
            spacing_before_lines: 0,
            blocks: vec![definition_list(vec![
                item("real-name"),
                item("bind-tty-special-chars (On)"),
                item("name prose"),
            ])],
            children: Vec::new(),
            source: None,
        },
    ];

    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

    let Block::DefinitionList {
        items: commands, ..
    } = &sections[0].blocks[0]
    else {
        panic!("commands");
    };
    assert_eq!(
        commands[0].entry.as_ref().expect("command").names,
        ["Send Env"]
    );
    assert!(
        commands[1]
            .entry
            .as_ref()
            .expect("unstyled prose")
            .names
            .is_empty()
    );
    assert_eq!(
        commands[2].entry.as_ref().expect("command form").names,
        ["bind"]
    );
    assert_eq!(
        commands[3].entry.as_ref().expect("command form").names,
        ["set"]
    );
    assert!(
        commands[4]
            .entry
            .as_ref()
            .expect("numeric prose")
            .names
            .is_empty()
    );
    let Block::DefinitionList {
        items: variables, ..
    } = &sections[1].blocks[0]
    else {
        panic!("variables");
    };
    assert_eq!(
        variables[0].entry.as_ref().expect("variable").names,
        ["real-name"]
    );
    assert!(
        variables[1]
            .entry
            .as_ref()
            .is_some_and(|identity| identity.names == ["bind-tty-special-chars"])
    );
    assert!(
        variables[2]
            .entry
            .as_ref()
            .expect("unclassified term")
            .names
            .is_empty()
    );
}

#[test]
fn colliding_generated_ids_follow_semantics_not_sibling_order() {
    fn ids(terms: &[&str], with_colliding_section: bool) -> HashMap<String, String> {
        let mut sections = Vec::new();
        if with_colliding_section {
            sections.push(Section {
                id: "option-v".into(),
                fragment_aliases: Vec::new(),
                heading: heading("Unrelated notes"),
                spacing_before_lines: 0,
                blocks: Vec::new(),
                children: Vec::new(),
                source: None,
            });
        }
        sections.push(Section {
            id: "options".into(),
            fragment_aliases: Vec::new(),
            heading: heading("OPTIONS"),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: terms.iter().map(|term| item(term)).collect(),
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
            children: Vec::new(),
            source: None,
        });

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
        let Block::DefinitionList { items, .. } = &sections.last().expect("options").blocks[0]
        else {
            panic!("definitions");
        };
        items
            .iter()
            .map(|item| {
                let identity = item.entry.as_ref().expect("identity");
                (identity.names[0].clone(), identity.id.to_string())
            })
            .collect()
    }

    let original = ids(&["-v", "-V"], false);
    let reordered = ids(&["-V", "-v"], false);
    let with_section = ids(&["-v", "-V"], true);
    assert_eq!(original, reordered);
    assert_eq!(original, with_section);
    assert_ne!(original["-v"], original["-V"]);
    assert!(original.values().all(|id| id.starts_with("option-v-")));
}

#[test]
fn normalizes_hanging_option_layout_before_assigning_identity() {
    let paragraph = |value: &str, indent_columns, spacing_before_lines| Block::Paragraph {
        children: vec![fixture::text(value)],
        layout: LayoutHint {
            indent_columns,
            spacing_before_lines,
            ..Default::default()
        },
        source: None,
    };
    let mut sections = vec![Section {
        id: "options".to_owned().into(),
        fragment_aliases: Vec::new(),
        heading: heading("OPTIONS"),
        spacing_before_lines: 0,
        blocks: vec![
            paragraph("-v, --version", 0, 1),
            paragraph("Print version information.", 4, 0),
            paragraph("-C <path>", 0, 1),
            paragraph("Run from path.", 4, 0),
        ],
        children: Vec::new(),
        source: None,
    }];

    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

    assert_eq!(sections[0].blocks.len(), 2);
    let Block::DefinitionList { items, layout, .. } = &sections[0].blocks[0] else {
        panic!("hanging option should become a definition list");
    };
    assert_eq!(layout.indent_columns, 0);
    assert_eq!(
        items[0].entry.as_ref().expect("option identity").names,
        ["-v", "--version"]
    );
    assert!(matches!(
        &items[0].description[0],
        Block::Paragraph { layout, .. }
            if layout.indent_columns == 0 && layout.spacing_before_lines == 0
    ));
    assert_eq!(items[0].layout.spacing_before_lines, Some(1));
    let Block::DefinitionList { items, .. } = &sections[0].blocks[1] else {
        panic!("second option should remain independently addressable");
    };
    assert_eq!(
        items[0].entry.as_ref().expect("option identity").names,
        ["-C"]
    );
}

#[test]
fn normalizes_complete_cross_platform_hanging_environment_assignments() {
    let paragraph = |value: &str, indent_columns| Block::Paragraph {
        children: vec![fixture::text(value)],
        layout: LayoutHint {
            indent_columns,
            spacing_before_lines: 0,
            ..Default::default()
        },
        source: None,
    };
    let mut sections = vec![Section {
        id: "environment".into(),
        fragment_aliases: Vec::new(),
        heading: heading("ENVIRONMENT VARIABLES"),
        spacing_before_lines: 0,
        blocks: vec![
            paragraph("HOME", 0),
            paragraph("User home.", 4),
            paragraph("$Env:Path = C:\\Tools", 0),
            paragraph("PowerShell provider form.", 4),
            paragraph("%ProgramFiles(x86)%=C:\\Program Files (x86)", 0),
            paragraph("Windows expansion form.", 4),
        ],
        children: Vec::new(),
        source: None,
    }];

    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
    // A bare HOME paragraph plus indentation is not independent declaration
    // evidence. Complete provider assignments retain their own owners.
    assert_eq!(sections[0].blocks.len(), 4);
    assert!(matches!(sections[0].blocks[0], Block::Paragraph { .. }));
    assert!(matches!(sections[0].blocks[1], Block::Paragraph { .. }));
    let identities = sections[0]
        .blocks
        .iter()
        .skip(2)
        .map(|block| {
            let Block::DefinitionList { items, .. } = block else {
                panic!("hanging environment entry should become a definition list");
            };
            items[0].entry.as_ref().expect("environment identity")
        })
        .collect::<Vec<_>>();
    assert_eq!(identities.len(), 2);
    assert!(
        identities
            .iter()
            .all(|identity| identity.kind == EntryKind::EnvironmentVariable)
    );
    assert_eq!(identities[0].names, ["$Env:Path"]);
    assert_eq!(identities[1].names, ["%ProgramFiles(x86)%"]);
    assert_eq!(identities[0].id.as_str(), "environment-path");
    assert_eq!(identities[1].id.as_str(), "environment-programfiles-x86");
}

#[test]
fn keeps_native_navigation_anchors_separate_from_semantic_ids() {
    let mut command = item("set-mark");
    command.terms[0].insert(0, fixture::anchor("set"));
    let mut sections = vec![Section {
        id: "commands".into(),
        fragment_aliases: Vec::new(),
        heading: heading("COMMANDS"),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![command],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    }];

    let retained = identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
    let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
        panic!("command definition list");
    };
    let identity = items[0].entry.as_ref().expect("command identity");
    assert_eq!(identity.id.as_str(), "command-set-mark");
    assert_eq!(identity.names, ["set-mark"]);
    assert!(retained.contains("set"));
    assert!(retained.contains("command-set-mark"));
}

#[test]
fn generic_terms_receive_the_anchor_their_projected_entry_advertises() {
    let mut sections = vec![Section {
        id: "glossary".into(),
        fragment_aliases: Vec::new(),
        heading: heading("GLOSSARY"),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![item("widget")],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    }];

    let retained = identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
    let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
        panic!("term definition list");
    };
    let identity = items[0].entry.as_ref().expect("term identity");
    assert_eq!(identity.id.as_str(), "term-widget");
    assert!(matches!(
        items[0].terms[0].first(),
        Some(Inline::Anchor { id, .. }) if id == "term-widget"
    ));
    assert!(retained.contains("term-widget"));
}

#[test]
fn qualified_technical_terms_are_addressable_without_colon_widening() {
    let mut sections = vec![Section {
        id: "modules".into(),
        fragment_aliases: Vec::new(),
        heading: heading("MODULES"),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![item("Class::ISA"), item("Pod::Plainer")],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    }];
    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
    let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
        panic!("qualified definition list");
    };
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["Class::ISA"]);
    assert_eq!(items[1].entry.as_ref().unwrap().names, ["Pod::Plainer"]);
}

#[test]
fn generic_terms_bind_complete_invocation_heads_and_optional_parameters() {
    let mut sections = vec![Section {
        id: "definitions".into(),
        fragment_aliases: Vec::new(),
        heading: heading("DEFINITIONS"),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![
                item("istrip[=<bool>]"),
                item("getservbyname NAME,PROTO"),
                item("Using References"),
            ],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    }];
    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
    let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
        panic!("generic definition list");
    };
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["istrip"]);
    assert_eq!(items[1].entry.as_ref().unwrap().names, ["getservbyname"]);
    assert!(items[2].entry.as_ref().unwrap().names.is_empty());
}

#[test]
fn retained_ordinal_labels_are_presentation_not_semantic_entries() {
    // Native man `.IP`/`.TP` labels may remain definitions when one item or
    // an authored marker spelling cannot prove an ordered list. They remain
    // visible for layout, without creating empty-name Term entries in
    // outlines, explain, or search.
    for label in ["1.", "2)", "(3)", "[4]"] {
        let mut sections = vec![Section {
            id: "notes".into(),
            fragment_aliases: Vec::new(),
            heading: heading("NOTES"),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![item(label)],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
            children: Vec::new(),
            source: None,
        }];

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
        let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
            panic!("expected retained ordinal definition");
        };
        assert!(items[0].entry.is_none(), "{label} became a semantic entry");
        assert_eq!(
            mant_ir::inline_plain_text(fixture::content(), &items[0].terms[0]),
            label
        );
    }

    // Ordinary literal labels remain eligible for generic-term discovery.
    let mut sections = vec![Section {
        id: "glossary".into(),
        fragment_aliases: Vec::new(),
        heading: heading("GLOSSARY"),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![item("ISO")],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
        children: Vec::new(),
        source: None,
    }];
    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
    let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
        panic!("expected ordinary term definition");
    };
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["ISO"]);
}

#[test]
fn classifies_environment_configuration_and_nested_parameter_semantics() {
    fn identities(section: &Section) -> Vec<&mant_ir::EntryFacts> {
        let Block::DefinitionList { items, .. } = &section.blocks[0] else {
            panic!("expected definition list");
        };
        items
            .iter()
            .map(|item| item.entry.as_ref().expect("semantic identity"))
            .collect()
    }

    let definition_list = |items| Block::DefinitionList {
        declaration_groups: Vec::new(),
        items,
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    };
    let section = |id: &str, title: &str, items| Section {
        id: id.into(),
        fragment_aliases: Vec::new(),
        heading: heading(title),
        spacing_before_lines: 0,
        blocks: vec![definition_list(items)],
        children: Vec::new(),
        source: None,
    };
    let mut option = item("-o MODE");
    option
        .description
        .push(definition_list(vec![item("yes"), item("no")]));
    let mut sections = vec![
        section("environment", "ENVIRONMENT", vec![item("PATH")]),
        section(
            "configuration",
            "CONFIGURATION KEYWORDS",
            vec![item("HostKeyAlgorithms")],
        ),
        section("options", "OPTIONS", vec![item("--"), item("-"), option]),
    ];

    identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

    assert_eq!(
        identities(&sections[0])[0].kind,
        EntryKind::EnvironmentVariable
    );
    assert_eq!(
        identities(&sections[1])[0].kind,
        EntryKind::ConfigurationKey
    );
    let parameters = identities(&sections[2]);
    assert_eq!(
        parameters[0].kind,
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Marker
        }
    );
    assert_eq!(
        parameters[1].kind,
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Operand
        }
    );
    assert_eq!(
        parameters[2].kind,
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option
        }
    );
    let Block::DefinitionList { items, .. } = &sections[2].blocks[0] else {
        panic!("expected option definitions");
    };
    let Block::DefinitionList { items: values, .. } = &items[2].description[0] else {
        panic!("expected nested values");
    };
    // The exact nested `--mode`/`fast`/`--other` shape in EN00 terms.1 was
    // run through pinned CVS -Tutf8 first. man_term.c::pre_TP and pre_RS
    // establish indentation and owner boundaries, not a Value role. Without
    // additional local value-declaration evidence, nested names remain Term.
    assert!(values.iter().all(|value| {
        value
            .entry
            .as_ref()
            .is_some_and(|identity| identity.kind == EntryKind::Term)
    }));
}

#[test]
fn ordinal_labels_are_not_semantic_values() {
    for marker in ["1.", "2)", "(3)", "[4]"] {
        assert!(!super::is_value_name(marker), "accepted {marker}");
    }
    for value in ["0", "1", "2.2", "c++", "default"] {
        assert!(super::is_value_name(value), "rejected {value}");
    }
}
