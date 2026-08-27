//! Tests for Fedora Linux 44's `sh(1)` alias of the Bash manual.

use mant_engine::{render_excerpt_markdown, select_explanation};
use mant_ir::{EntryKind, ParameterKind, SemanticEntry, SemanticIndex, SourceFormat, ValueDomain};

use crate::common::{self, collect_sections, source_path_ends_with};
use crate::fixtures::fedora44_manual;

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
            entry.kind == EntryKind::Command && entry.aliases.iter().any(|alias| alias == "set")
        })
        .expect("the set builtin is a semantic command");

    assert!(has_parameter(set, ParameterKind::Marker, "--"));
    assert!(has_parameter(set, ParameterKind::Operand, "-"));
    assert!(has_parameter(set, ParameterKind::Option, "-o"));
    let named_option = set
        .children
        .iter()
        .find(|entry| entry.aliases.iter().any(|alias| alias == "-o"))
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
                    && entry.aliases.iter().any(|alias| alias == "-O")
                    && entry.aliases.iter().any(|alias| alias == "+O")
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
        .flat_map(|entry| entry.aliases.iter().map(String::as_str))
        .collect::<Vec<_>>();

    for name in [
        "operate-and-get-next",
        "edit-and-execute-command",
        "re-read-init-file",
        "do-lowercase-version",
        "character-search-backward",
    ] {
        assert!(aliases.contains(&name), "missing Readline command {name}");
    }

    let query = common::query_for_document("sh", document);
    let excerpt = select_explanation(&query, "operate-and-get-next")
        .expect("full Readline command alias is explainable");
    let markdown = render_excerpt_markdown(&excerpt);
    assert!(markdown.contains("operate-and-get-next"));
    assert!(markdown.contains("fetch the next line"));
}

fn has_parameter(entry: &SemanticEntry, parameter_kind: ParameterKind, alias: &str) -> bool {
    entry.children.iter().any(|child| {
        child.kind == EntryKind::Parameter { parameter_kind }
            && child.aliases.iter().any(|candidate| candidate == alias)
    })
}

fn all_entries(entries: &[SemanticEntry]) -> Box<dyn Iterator<Item = &SemanticEntry> + '_> {
    Box::new(
        entries
            .iter()
            .flat_map(|entry| std::iter::once(entry).chain(all_entries(&entry.children))),
    )
}
