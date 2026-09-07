//! Identify definitions in explicit preparation, counting and allocation passes.
//! Source-neutral topology is stable before identities are allocated.

mod context;
mod diagnostics;
mod identity;
mod normalize;
mod syntax;
mod walk;

use context::{DefinitionContext, child_definition_context, definition_group_context};
pub(crate) use diagnostics::manual_discovery_diagnostics;
pub(crate) use identity::document_id_slug;
use identity::{
    document_anchor_ids, has_semantic_spelling, identify_item, identify_list_item, identity_plan,
    list_identity_base,
};
use mant_ir::{Block, Section};
use normalize::{normalize_definition_nesting, normalize_hanging_definitions};
use std::collections::{HashMap, HashSet};
pub(crate) use syntax::{
    environment_variable_alias, environment_variable_body, option_names_from_terms, option_prefix,
    slash_option_forms,
};
#[cfg(test)]
use syntax::{is_value_name, option_names};
pub(crate) use walk::{ContentEntry, content_entries};

/// Annotate reliably recognizable command-line options and return every
/// inline anchor that the navigation resolver must retain.
pub(crate) fn identify_definitions(
    blocks: &mut Vec<Block>,
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
    document_name: Option<&str>,
) -> HashSet<String> {
    let mut preferred_counts = HashMap::new();
    let root_context = document_name.map_or(DefinitionContext::Generic, |name| {
        let name = name.to_ascii_lowercase();
        if name.ends_with("_config") || name.ends_with("-config") {
            DefinitionContext::ConfigurationKeys
        } else {
            DefinitionContext::Generic
        }
    });
    prepare_blocks(blocks, root_context, &mut preferred_counts);
    prepare_sections(sections, root_context, &mut preferred_counts);

    let used = document_anchor_ids(blocks, sections);
    let mut discovery = DefinitionDiscovery {
        retained: used.clone(),
        used,
        reserved: reserved_targets,
        preferred_counts: &preferred_counts,
    };
    discovery.identify_blocks(blocks, root_context);
    for section in sections {
        let context = DefinitionContext::for_section(&section.title, root_context);
        discovery.identify_blocks(&mut section.blocks, context);
        discovery.identify_sections(&mut section.children, context);
    }
    discovery.retained
}

fn prepare_sections(
    sections: &mut [Section],
    parent_context: DefinitionContext,
    preferred_counts: &mut HashMap<String, usize>,
) {
    for section in sections {
        let context = DefinitionContext::for_section(&section.title, parent_context);
        prepare_blocks(&mut section.blocks, context, preferred_counts);
        prepare_sections(&mut section.children, context, preferred_counts);
    }
}

fn prepare_blocks(
    blocks: &mut Vec<Block>,
    context: DefinitionContext,
    preferred_counts: &mut HashMap<String, usize>,
) {
    normalize_definition_nesting(blocks);
    normalize_hanging_definitions(blocks, context);
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    if let Some(preferred) = list_identity_base(item) {
                        *preferred_counts.entry(preferred).or_default() += 1;
                    }
                    prepare_blocks(&mut item.blocks, context, preferred_counts);
                }
            }
            Block::DefinitionList { items, .. } => {
                let item_context = definition_group_context(items, context);
                for item in items {
                    let plan = identity_plan(item, item_context);
                    if has_semantic_spelling(item, &plan) {
                        *preferred_counts.entry(plan.preferred).or_default() += 1;
                    }
                    let child_context = child_definition_context(plan.role, item_context);
                    prepare_blocks(&mut item.description, child_context, preferred_counts);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        prepare_blocks(&mut cell.blocks, context, preferred_counts);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

struct DefinitionDiscovery<'a> {
    used: HashSet<String>,
    reserved: &'a HashSet<String>,
    retained: HashSet<String>,
    preferred_counts: &'a HashMap<String, usize>,
}

impl DefinitionDiscovery<'_> {
    fn identify_sections(&mut self, sections: &mut [Section], parent_context: DefinitionContext) {
        for section in sections {
            let context = DefinitionContext::for_section(&section.title, parent_context);
            self.identify_blocks(&mut section.blocks, context);
            self.identify_sections(&mut section.children, context);
        }
    }

    fn identify_blocks(&mut self, blocks: &mut [Block], context: DefinitionContext) {
        for block in blocks {
            match block {
                Block::List { items, .. } => {
                    for item in items {
                        let role = identify_list_item(
                            item,
                            &mut self.used,
                            self.reserved,
                            &mut self.retained,
                            self.preferred_counts,
                        );
                        let context =
                            role.map_or(context, |role| child_definition_context(role, context));
                        self.identify_blocks(&mut item.blocks, context);
                    }
                }
                Block::DefinitionList { items, .. } => {
                    let item_context = definition_group_context(items, context);
                    for item in items {
                        let role = identify_item(
                            item,
                            item_context,
                            &mut self.used,
                            self.reserved,
                            &mut self.retained,
                            self.preferred_counts,
                        );
                        let child_context = child_definition_context(role, item_context);
                        self.identify_blocks(&mut item.description, child_context);
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &mut row.cells {
                            self.identify_blocks(&mut cell.blocks, context);
                        }
                    }
                }
                Block::Paragraph { .. }
                | Block::Preformatted { .. }
                | Block::Equation { .. }
                | Block::VerticalSpace { .. }
                | Block::ThematicBreak { .. }
                | Block::Unsupported { .. } => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use mant_ir::{
        Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Inline,
        LayoutHint, Section,
    };

    use super::{environment_variable_alias, identify_definitions, option_names, option_prefix};

    fn item(value: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            inline_term: false,
            terms: vec![vec![Inline::Text {
                value: value.into(),
            }]],
            description: Vec::new(),
            spacing_before_lines: None,
        }
    }

    fn strong_item(value: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            inline_term: false,
            terms: vec![vec![Inline::Strong {
                children: vec![Inline::Text {
                    value: value.into(),
                }],
            }]],
            description: Vec::new(),
            spacing_before_lines: None,
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
    fn semantic_id_allocation_ignores_a_prefilled_producer_id() {
        let mut option = item("--verbose");
        option.identity = Some(DefinitionIdentity {
            name_bindings: Vec::new(),
            alias_groups: Vec::new(),
            alias_of: None,
            forms: Vec::new(),
            id: "producer-specific-id".into(),
            role: DefinitionRole::Option,
            case: DefinitionCase::Sensitive,
            names: vec!["--verbose".to_owned()],
            value_domain: None,
        });
        let mut sections = vec![Section {
            id: "options".into(),
            fragment_aliases: Vec::new(),
            title: "OPTIONS".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
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
            items[0].identity.as_ref().expect("identity").id.as_str(),
            "option-verbose"
        );
    }

    #[test]
    fn target_only_definitions_retain_anchors_without_becoming_entries() {
        let target_only = DefinitionItem {
            identity: None,
            inline_term: true,
            terms: vec![vec![Inline::anchor("native-target")]],
            description: Vec::new(),
            spacing_before_lines: None,
        };
        let mut sections = vec![Section {
            id: "notes".into(),
            fragment_aliases: Vec::new(),
            title: "NOTES".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
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
        assert!(items[0].identity.is_none());
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
            title: "ENVIRONMENT OPTIONS".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
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
        assert_eq!(
            items[0].identity.as_ref().expect("term").role,
            DefinitionRole::Term
        );
        assert_eq!(
            items[1].identity.as_ref().expect("option").role,
            DefinitionRole::Option
        );
    }

    #[test]
    fn command_discovery_requires_a_structural_or_syntactic_boundary() {
        let definition_list = |items| Block::DefinitionList {
            items,
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        };
        let mut sections = vec![
            Section {
                id: "commands".into(),
                fragment_aliases: Vec::new(),
                title: "COMMANDS".to_owned(),
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
                title: "VARIABLES".to_owned(),
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
            commands[0].identity.as_ref().expect("command").names,
            ["Send Env"]
        );
        assert!(
            commands[1]
                .identity
                .as_ref()
                .expect("unstyled prose")
                .names
                .is_empty()
        );
        assert_eq!(
            commands[2].identity.as_ref().expect("command form").names,
            ["bind"]
        );
        assert_eq!(
            commands[3].identity.as_ref().expect("command form").names,
            ["set"]
        );
        assert!(
            commands[4]
                .identity
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
            variables[0].identity.as_ref().expect("variable").names,
            ["real-name"]
        );
        assert!(
            variables[1]
                .identity
                .as_ref()
                .is_some_and(|identity| identity.names == ["bind-tty-special-chars"])
        );
        assert!(
            variables[2]
                .identity
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
                    title: "Unrelated notes".to_owned(),
                    spacing_before_lines: 0,
                    blocks: Vec::new(),
                    children: Vec::new(),
                    source: None,
                });
            }
            sections.push(Section {
                id: "options".into(),
                fragment_aliases: Vec::new(),
                title: "OPTIONS".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![Block::DefinitionList {
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
                    let identity = item.identity.as_ref().expect("identity");
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
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines,
            },
            source: None,
        };
        let mut sections = vec![Section {
            id: "options".to_owned().into(),
            fragment_aliases: Vec::new(),
            title: "OPTIONS".to_owned(),
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
            items[0].identity.as_ref().expect("option identity").names,
            ["-v", "--version"]
        );
        assert!(matches!(
            &items[0].description[0],
            Block::Paragraph { layout, .. }
                if layout.indent_columns == 0 && layout.spacing_before_lines == 0
        ));
        assert_eq!(items[0].spacing_before_lines, Some(1));
        let Block::DefinitionList { items, .. } = &sections[0].blocks[1] else {
            panic!("second option should remain independently addressable");
        };
        assert_eq!(
            items[0].identity.as_ref().expect("option identity").names,
            ["-C"]
        );
    }

    #[test]
    fn normalizes_cross_platform_hanging_environment_definitions() {
        let paragraph = |value: &str, indent_columns| Block::Paragraph {
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines: 0,
            },
            source: None,
        };
        let mut sections = vec![Section {
            id: "environment".into(),
            fragment_aliases: Vec::new(),
            title: "ENVIRONMENT VARIABLES".to_owned(),
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

        let identities = sections[0]
            .blocks
            .iter()
            .map(|block| {
                let Block::DefinitionList { items, .. } = block else {
                    panic!("hanging environment entry should become a definition list");
                };
                items[0].identity.as_ref().expect("environment identity")
            })
            .collect::<Vec<_>>();
        assert_eq!(identities.len(), 3);
        assert!(
            identities
                .iter()
                .all(|identity| identity.role == DefinitionRole::EnvironmentVariable)
        );
        assert_eq!(identities[0].names, ["HOME"]);
        assert_eq!(identities[1].names, ["$Env:Path"]);
        assert_eq!(identities[2].names, ["%ProgramFiles(x86)%"]);
        assert_eq!(identities[1].id.as_str(), "environment-path");
        assert_eq!(identities[2].id.as_str(), "environment-programfiles-x86");
    }

    #[test]
    fn keeps_native_navigation_anchors_separate_from_semantic_ids() {
        let mut command = item("set-mark");
        command.terms[0].insert(0, Inline::anchor("set"));
        let mut sections = vec![Section {
            id: "commands".into(),
            fragment_aliases: Vec::new(),
            title: "COMMANDS".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
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
        let identity = items[0].identity.as_ref().expect("command identity");
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
            title: "GLOSSARY".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
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
        let identity = items[0].identity.as_ref().expect("term identity");
        assert_eq!(identity.id.as_str(), "term-widget");
        assert!(matches!(
            items[0].terms[0].first(),
            Some(Inline::Anchor { id, .. }) if id == "term-widget"
        ));
        assert!(retained.contains("term-widget"));
    }

    #[test]
    fn classifies_environment_configuration_and_nested_parameter_semantics() {
        fn identities(section: &Section) -> Vec<&mant_ir::DefinitionIdentity> {
            let Block::DefinitionList { items, .. } = &section.blocks[0] else {
                panic!("expected definition list");
            };
            items
                .iter()
                .map(|item| item.identity.as_ref().expect("semantic identity"))
                .collect()
        }

        let definition_list = |items| Block::DefinitionList {
            items,
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        };
        let section = |id: &str, title: &str, items| Section {
            id: id.into(),
            fragment_aliases: Vec::new(),
            title: title.to_owned(),
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
            identities(&sections[0])[0].role,
            DefinitionRole::EnvironmentVariable
        );
        assert_eq!(
            identities(&sections[1])[0].role,
            DefinitionRole::ConfigurationKey
        );
        let parameters = identities(&sections[2]);
        assert_eq!(parameters[0].role, DefinitionRole::Marker);
        assert_eq!(parameters[1].role, DefinitionRole::Operand);
        assert_eq!(parameters[2].role, DefinitionRole::Option);
        let Block::DefinitionList { items, .. } = &sections[2].blocks[0] else {
            panic!("expected option definitions");
        };
        let Block::DefinitionList { items: values, .. } = &items[2].description[0] else {
            panic!("expected nested values");
        };
        assert!(values.iter().all(|value| {
            value
                .identity
                .as_ref()
                .is_some_and(|identity| identity.role == DefinitionRole::Value)
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
}
