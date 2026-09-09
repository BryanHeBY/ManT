//! Presentation changes must preserve source facts and exact query evidence.
use mant_loader::{load_markdown_text, load_roff_bytes};
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery};
use mant_query::explain_query;

#[test]
fn formatter_counterexample_has_three_correct_owners_before_decoration() {
    let source = include_bytes!("fixtures/entry-presentation.1");
    let content = load_roff_bytes(source).unwrap();
    let result = explain_query(
        &content,
        &ExplanationQuery {
            entry: "-x".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.total, 3);
    for (evidence, class, range) in result
        .evidence
        .iter()
        .zip([
            (EvidenceClass::DirectEntry, (24, 26)),
            (EvidenceClass::EntryMention, (4, 6)),
            (EvidenceClass::ContextMention, (38, 40)),
        ])
        .map(|(e, (c, r))| (e, c, r))
    {
        assert_eq!(evidence.class, class);
        let preview = &evidence.previews[0];
        assert_eq!((preview.match_start_char, preview.match_end_char), range);
        assert_eq!(
            preview
                .text
                .chars()
                .skip(range.0 as usize)
                .take(2)
                .collect::<String>(),
            "-x"
        );
    }
}

#[test]
fn explicit_presentation_fixture_has_all_roles_without_diagnostics() {
    let content = load_markdown_text(include_str!("fixtures/entry-presentation.md"), None).unwrap();
    let document = content.document.as_ref().unwrap();
    assert!(
        document.diagnostics.is_empty(),
        "{:?}",
        document.diagnostics
    );
    let index = mant_ir::SemanticIndex::build(document);
    let kinds: std::collections::BTreeSet<_> = document
        .sections
        .iter()
        .flat_map(|s| index.section(&s.id))
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds.len(), 9);
}

#[test]
fn search_keeps_a_form_label_without_inventing_a_name() {
    let content =
        load_roff_bytes(b".TH PROBE 1\n.SH TERMS\n.TP\n.B ^find-new.*\nFind new files.\n").unwrap();
    let search = mant_query::search_query(
        &content,
        &mant_protocol::SearchQuery {
            pattern: "find-new".into(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Insensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 1,
            offset: 0,
        },
    )
    .unwrap();
    let node = &search.matches[0].outline.node;
    assert_eq!(search.matches[0].outline.title(), "^find-new.*");
    assert!(
        matches!(node, mant_protocol::OutlineNodeReference::DocumentEntry { names, .. } if names.is_empty())
    );
    let decoded: mant_protocol::QuerySearch =
        serde_json::from_str(&serde_json::to_string(&search).unwrap()).unwrap();
    assert_eq!(decoded, search);
}

#[test]
fn singular_command_headings_share_the_plural_semantic_contract() {
    for heading in ["COMMAND", "COMMANDS", "SUBCOMMAND", "SUBCOMMANDS"] {
        let source = format!(
            ".TH PROBE 1\n.SH {heading}\n.TP\n.B find-new <subvolume> <last_gen>\nFind new files.\n"
        );
        let content = load_roff_bytes(source.as_bytes()).unwrap();
        let document = content.document.as_ref().unwrap();
        let index = mant_ir::SemanticIndex::build(document);
        let entry = &index.section(&document.sections[0].id)[0];
        assert_eq!(entry.kind, mant_ir::EntryKind::Command, "{heading}");
        assert_eq!(entry.names, ["find-new"]);
        assert_eq!(entry.id.as_str(), "command-find-new");
        assert_eq!(entry.forms, ["find-new <subvolume> <last_gen>"]);
    }
}

#[test]
fn outline_keeps_full_titles_on_primary_lines_and_hangs_all_metadata() {
    let source = format!(
        "# Tool\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- `command {}`: Details.\n\n## Other\nText.\n",
        "ARG".repeat(60)
    );
    let content = load_markdown_text(&source, None).unwrap();
    let outline =
        mant_query::build_outline_projection(&content, mant_protocol::EntryProjection::All, None)
            .unwrap();
    let text = mant_render::render_outline_text(&outline);
    assert_eq!(
        text,
        mant_render::render_outline_text_with(&outline, |_, t| t.to_owned())
    );
    let title_line = text.lines().find(|l| l.contains("command ARG")).unwrap();
    assert!(title_line.contains(&"ARG".repeat(60)));
    assert!(!title_line.contains("ID:"));
    assert!(text.contains("│      ID: command-command"), "{text}");
    assert!(text.lines().any(|l| l.contains("Entries:")), "{text}");
    assert!(text.contains("└─ 2 Other\n     ID:"));
}
