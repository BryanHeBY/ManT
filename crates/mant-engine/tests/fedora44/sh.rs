//! Tests for Fedora Linux 44's `sh(1)` alias of the Bash manual.

use mant_engine::{ProjectionError, render_excerpt_markdown, select_excerpt};
use mant_ir::{EntryKind, ParameterKind, SemanticEntry, SemanticIndex, SourceFormat, ValueDomain};

use crate::common::{self, collect_sections, source_path_ends_with};
use crate::fixtures::fedora44_manual;

fn assert_builtin_evidence(query: &mant_ir::ResolvedContent, name: &str) {
    let explanation = mant_engine::explain_query(
        query,
        &mant_protocol::ExplanationQuery {
            entry: name.into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .unwrap();
    let direct = explanation
        .evidence
        .iter()
        .filter(|e| e.class == mant_protocol::EvidenceClass::DirectEntry)
        .collect::<Vec<_>>();
    assert!(!direct.is_empty(), "{name}");
    for evidence in direct {
        let entry = evidence.entry.as_ref().unwrap();
        assert_eq!(entry.kind, EntryKind::Command, "{name}");
        assert!(entry.names.iter().any(|candidate| candidate == name));
        assert!(
            evidence
                .outline
                .ancestors
                .iter()
                .any(|a| a.title == "SHELL BUILTIN COMMANDS")
        );
        let excerpt = select_excerpt(query, &[evidence.outline.path()]).unwrap();
        assert!(!render_excerpt_markdown(&excerpt).contains("set-mark (C-@"));
    }
}

#[test]
fn parses_the_real_bash_backed_shell_manual() {
    let document = fedora44_manual("sh");
    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(document.meta.manual_section.as_deref(), Some("1"));
    assert!(source_path_ends_with(document, "fedora44/sh.1.zst"));

    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    assert_eq!(document.sections.len(), 38);
    for title in ["NAME", "SHELL GRAMMAR", "REDIRECTION", "FUNCTIONS"] {
        assert!(sections.iter().any(|section| section.title == title));
    }
}

#[test]
fn keeps_the_bash_shell_page_spacing_and_anchors_normalized() {
    let document = fedora44_manual("sh");
    common::assert_anchor_ids_are_clean("fedora44/sh", document);
    common::assert_no_duplicate_vertical_spacing(&document.sections, "fedora44/sh");
}

#[test]
fn rebuilds_builtin_parameter_hierarchy_from_relative_indentation() {
    let document = fedora44_manual("sh");
    let index = SemanticIndex::build(document);
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    let set = sections
        .iter()
        .flat_map(|section| index.section(&section.id))
        .find(|entry| {
            entry.kind == EntryKind::Command
                && entry.names.iter().any(|alias| alias == "set")
                && !entry.children.is_empty()
        })
        .expect("the set builtin is a semantic command");

    assert!(has_parameter(set, ParameterKind::Marker, "--"));
    assert!(has_parameter(set, ParameterKind::Operand, "-"));
    assert!(has_parameter(set, ParameterKind::Option, "-o"));
    let named_option = set
        .children
        .iter()
        .find(|entry| entry.names.iter().any(|alias| alias == "-o"))
        .expect("set -o parameter");
    assert!(matches!(
        named_option.value_domain,
        Some(ValueDomain::Choices { exhaustive: false })
    ));
    assert!(
        named_option
            .children
            .iter()
            .all(|entry| entry.kind == EntryKind::Value)
    );
    assert!(
        sections
            .iter()
            .flat_map(|section| all_entries(index.section(&section.id)))
            .any(|entry| {
                entry.kind
                    == (EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    })
                    && entry.names.iter().any(|alias| alias == "-O")
                    && entry.names.iter().any(|alias| alias == "+O")
            })
    );
}

#[test]
fn preserves_complete_readline_command_names_as_selectable_aliases() {
    let document = fedora44_manual("sh");
    let index = SemanticIndex::build(document);
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    let aliases = sections
        .iter()
        .filter(|section| {
            matches!(
                section.title.as_str(),
                "Commands for Manipulating the History" | "Miscellaneous"
            )
        })
        .flat_map(|section| all_entries(index.section(&section.id)))
        .filter(|entry| entry.kind == EntryKind::Command)
        .flat_map(|entry| entry.names.iter().map(String::as_str))
        .collect::<Vec<_>>();

    for name in [
        "operate-and-get-next",
        "edit-and-execute-command",
        "re-read-init-file",
        "do-lowercase-version",
        "character-search-backward",
    ] {
        assert!(
            aliases.contains(&name),
            "missing Readline command {name}; discovered {aliases:?}"
        );
    }

    let query = common::query_for_document("sh", document);
    let excerpt = select_excerpt(&query, &["operate-and-get-next"])
        .expect("full Readline command alias is explainable");
    let markdown = render_excerpt_markdown(&excerpt);
    assert!(markdown.contains("operate-and-get-next"));
    assert!(markdown.contains("fetch the next line"));

    let set_mark = sections
        .iter()
        .flat_map(|section| all_entries(index.section(&section.id)))
        .find(|entry| entry.names.iter().any(|alias| alias == "set-mark"))
        .expect("set-mark Readline command");
    assert_eq!(set_mark.id.as_str(), "command-set-mark");
    assert!(
        select_excerpt(&query, &["command-set-mark"]).is_ok(),
        "generated role-qualified ID must select set-mark"
    );

    assert_builtin_evidence(&query, "set");
}

#[test]
fn discovers_styled_builtin_names_without_promoting_argument_prose() {
    let document = fedora44_manual("sh");
    let index = SemanticIndex::build(document);
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    let entries = sections
        .iter()
        .flat_map(|section| all_entries(index.section(&section.id)))
        .collect::<Vec<_>>();

    for name in ["let", "test", "getopts", "builtin"] {
        assert!(
            entries.iter().any(|entry| {
                entry.kind == EntryKind::Command && entry.names.iter().any(|alias| alias == name)
            }),
            "missing styled shell builtin {name}"
        );
    }
    assert!(
        entries
            .iter()
            .all(|entry| entry.names.iter().all(|alias| alias != "0 arguments")),
        "descriptive prose below the command section must remain unclassified"
    );

    let query = common::query_for_document("sh", document);
    for name in ["let", "test", "getopts"] {
        let excerpt = select_excerpt(&query, &[name])
            .unwrap_or_else(|error| panic!("builtin {name} must be explainable: {error}"));
        assert!(
            render_excerpt_markdown(&excerpt).contains("SHELL BUILTIN COMMANDS"),
            "{name} resolved outside the builtin section"
        );
    }
    assert!(
        matches!(
            select_excerpt(&query, &["builtin"]),
            Err(ProjectionError::AmbiguousSelector { .. })
        ),
        "a command/value collision must be explicit instead of silently choosing one entry"
    );
}

#[test]
fn preserves_complete_readline_variable_names_without_shadowing_builtins() {
    let document = fedora44_manual("sh");
    let index = SemanticIndex::build(document);
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    let readline = sections
        .iter()
        .find(|section| section.title == "Readline Variables")
        .expect("Readline Variables section");
    let variables = index.section(&readline.id);

    assert_eq!(variables.len(), 49);
    assert!(
        variables
            .iter()
            .all(|entry| entry.kind == EntryKind::Variable)
    );
    for name in [
        "bind-tty-special-chars",
        "echo-control-characters",
        "enable-active-region",
        "horizontal-scroll-mode",
        "isearch-terminators",
        "keyseq-timeout",
    ] {
        assert!(
            variables
                .iter()
                .any(|entry| entry.names.iter().any(|alias| alias == name)),
            "missing complete Readline variable {name}"
        );
    }
    for prefix in ["bind", "echo", "enable", "set"] {
        assert!(
            variables
                .iter()
                .all(|entry| entry.names.iter().all(|alias| alias != prefix)),
            "Readline variables must not expose the short prefix {prefix}"
        );
    }

    let query = common::query_for_document("sh", document);
    for name in [
        "horizontal-scroll-mode",
        "isearch-terminators",
        "keyseq-timeout",
    ] {
        assert!(
            select_excerpt(&query, &[name]).is_ok(),
            "complete Readline variable {name} must be explainable"
        );
    }
    for builtin in ["bind", "echo", "enable", "set"] {
        assert_builtin_evidence(&query, builtin);
    }
    assert!(matches!(
        select_excerpt(&query, &["history"])
            .unwrap()
            .selections
            .as_slice(),
        [mant_protocol::ExcerptSelection::DocumentSection { .. }]
    ));
    assert!(matches!(
        select_excerpt(&query, &["complete"]),
        Err(ProjectionError::AmbiguousSelector { .. })
    ));
}

#[test]
fn preserves_compact_invocations_without_borrowing_the_next_description() {
    let document = fedora44_manual("sh");
    let index = SemanticIndex::build(document);
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    let aliases = ["--init-file", "--rcfile"];
    let entries = aliases.map(|alias| {
        sections
            .iter()
            .flat_map(|section| all_entries(index.section(&section.id)))
            .find(|entry| entry.names.iter().any(|candidate| candidate == alias))
            .unwrap_or_else(|| panic!("missing invocation option {alias}"))
    });

    assert_ne!(entries[0].id, entries[1].id);

    let query = common::query_for_document("sh", document);
    for alias in aliases {
        let excerpt = select_excerpt(&query, &[alias])
            .unwrap_or_else(|error| panic!("{alias} must be explainable: {error}"));
        let rendered = render_excerpt_markdown(&excerpt);
        assert!(rendered.contains(alias), "{rendered}");
        assert_eq!(
            rendered.contains("Execute commands from"),
            alias == "--rcfile",
            "{rendered}"
        );
    }
}

#[test]
fn explanation_preserves_history_builtin_and_nested_value_as_independent_evidence() {
    let query = crate::common::query_for_document("sh", fedora44_manual("sh"));
    let result = mant_engine::explain_query(
        &query,
        &mant_protocol::ExplanationQuery {
            entry: "history".into(),
            options: mant_protocol::ExplanationOptions {
                limit: 256,
                ..mant_protocol::ExplanationOptions::default()
            },
        },
    )
    .unwrap();
    let named = result
        .evidence
        .iter()
        .filter(|evidence| {
            evidence
                .bases
                .iter()
                .any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Name { .. }))
        })
        .collect::<Vec<_>>();
    for role in [mant_ir::EntryKind::Command, mant_ir::EntryKind::Value] {
        assert!(
            named.iter().any(|evidence| evidence
                .entry
                .as_ref()
                .is_some_and(|entry| entry.kind == role && entry.names == ["history"])),
            "missing {role:?}"
        );
    }
    assert!(
        named
            .iter()
            .all(|evidence| evidence.entry.as_ref().unwrap().alias_groups.is_empty()),
        "shared native names are not declared equivalence"
    );
    for evidence in named {
        assert!(mant_engine::select_excerpt(&query, &[evidence.outline.path()]).is_ok());
    }
}

fn has_parameter(entry: &SemanticEntry, parameter_kind: ParameterKind, alias: &str) -> bool {
    entry.children.iter().any(|child| {
        child.kind == EntryKind::Parameter { parameter_kind }
            && child.names.iter().any(|candidate| candidate == alias)
    })
}

fn all_entries(entries: &[SemanticEntry]) -> Box<dyn Iterator<Item = &SemanticEntry> + '_> {
    Box::new(
        entries
            .iter()
            .flat_map(|entry| std::iter::once(entry).chain(all_entries(&entry.children))),
    )
}
