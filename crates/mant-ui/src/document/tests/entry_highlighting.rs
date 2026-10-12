//! Authored IR tests keep signature decoration independent of parsing policy.
use super::*;
use mant_ir::{
    EntryContentSlice, EntryForm, EntryInlineRoot, EntryNameBinding, EntryNameEvidence,
    ParameterKind,
};

const LABEL: &str = "--output[=<文件e\u{301}👩‍💻>] [fast|slow]";

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn content() -> ResolvedContent {
    let mut content = bundle();
    let item = DefinitionItem {
        head_body_relation: mant_ir::HeadBodyRelation::from(false),
        terms: vec![
            vec![Inline::Strong {
                children: vec![
                    text("--output"),
                    text("[="),
                    Inline::Link {
                        target: mant_ir::LinkTarget::Section {
                            id: "description".into(),
                        },
                        title: None,
                        children: vec![
                            text("<"),
                            Inline::Emphasis {
                                children: vec![text("文件e\u{301}👩‍💻")],
                            },
                            text(">"),
                        ],
                    },
                    text("] [fast|slow]"),
                ],
            }]
            .into(),
        ],
        description: vec![Block::Paragraph {
            children: vec![text(LABEL)],
            inline_layout: mant_ir::InlineLayout::default(),
            layout: LayoutHint::default(),
            source: None,
        }],
        entry: Some(EntryFacts {
            id: "output".into(),
            kind: EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            case: NameCase::Sensitive,
            names: vec!["--output".into()],
            alias_groups: vec![],
            alias_of: None,
            value_domain: None,
            forms: vec![EntryForm::term(0)],
            name_bindings: vec![EntryNameBinding {
                name: 0,
                evidence: EntryNameEvidence::Declared,
                occurrences: vec![EntryForm {
                    parts: vec![EntryContentSlice {
                        root: EntryInlineRoot::Term { index: 0 },
                        path: vec![0, 0],
                        bytes: None,
                    }],
                }],
            }],
        }),
        layout: mant_ir::DefinitionLayout::default(),
        source: None,
    };
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::DefinitionList {
        items: vec![item],
        compact: true,
        declaration_groups: vec![],
        layout: LayoutHint::default(),
        source: None,
    }];
    content
}

fn item(content: &mut ResolvedContent) -> &mut DefinitionItem {
    let Block::DefinitionList { items, .. } =
        &mut content.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        panic!("definition")
    };
    &mut items[0]
}

fn style_at(line: &Line<'_>, byte: usize) -> Style {
    let mut offset = 0;
    for span in &line.spans {
        let end = offset + span.content.len();
        if (offset..end).contains(&byte) {
            return span.style;
        }
        offset = end;
    }
    panic!("missing byte {byte}")
}

#[test]
fn source_bound_signature_accents_preserve_names_modifiers_and_body_text() {
    let content = content();
    assert_eq!(
        mant_ir::validate_document(content.document.as_ref().unwrap()),
        []
    );
    let before = content.clone();
    let view = DocumentView::new(&content);
    let rendered = view.render(120);
    let heads = rendered.search(LABEL);
    assert_eq!(heads.len(), 2);
    let head = &rendered.text.lines[heads[0].row];
    let visible = head.to_string();
    let start = visible.find(LABEL).unwrap();
    assert_eq!(style_at(head, start).fg, Some(theme::GREEN));
    for (byte, character) in LABEL.char_indices() {
        let style = style_at(head, start + byte);
        assert!(style.add_modifier.contains(Modifier::BOLD));
        match character {
            '[' | ']' | '<' | '>' | '|' => assert_eq!(style.fg, Some(theme::MAUVE)),
            '=' => assert_eq!(style.fg, Some(theme::BLUE)),
            _ => {}
        }
    }
    let value = style_at(head, start + LABEL.find('文').unwrap());
    assert!(
        value
            .add_modifier
            .contains(Modifier::ITALIC | Modifier::UNDERLINED)
    );
    for marker in ['<', '>'] {
        assert!(
            style_at(head, start + LABEL.find(marker).unwrap())
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }
    let body = &rendered.text.lines[heads[1].row];
    assert!(
        body.spans
            .iter()
            .filter(|span| !span.content.trim().is_empty())
            .all(|span| span.style.fg == Some(theme::TEXT))
    );
    assert_eq!(content, before);
}

#[test]
fn signature_reflow_keeps_link_copy_search_and_navigation_coordinates() {
    let view = DocumentView::new(&content());
    let label = "文件e\u{301}👩‍💻";
    for width in [12, 24, 120, 12] {
        let rendered = view.render(width);
        let hit = &rendered.search(label)[0];
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&LinkTarget::Section("description".into()))
        );
        let (row, column) = hit
            .additional_fragments
            .last()
            .map_or((hit.row, hit.end_column), |fragment| {
                (fragment.row, fragment.end_column)
            });
        let copied = rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: hit.row,
                column: hit.start_column,
            },
            focus: TextPosition {
                row,
                column: column - 1,
            },
        });
        assert_eq!(
            copied.lines().map(str::trim_start).collect::<String>(),
            label
        );
        assert!(rendered.anchor_row("output").is_some());
        assert_eq!(rendered.text, view.render(width).text);
    }
}

#[test]
fn invalid_unrecorded_generic_and_oversized_forms_do_not_gain_accents() {
    for mode in ["invalid", "unrecorded", "term", "oversized"] {
        let mut content = content();
        let item = item(&mut content);
        let facts = item.entry.as_mut().unwrap();
        match mode {
            "invalid" => facts.forms.push(EntryForm::term(9)),
            "unrecorded" => {
                facts.forms.clear();
                facts.names.clear();
                facts.name_bindings.clear();
            }
            "term" => facts.kind = EntryKind::Term,
            _ => {
                item.terms[0] = vec![text(&format!(
                    "--output=<{}>",
                    "x".repeat(crate::entry::MAX_FORM_BYTES)
                ))]
                .into();
            }
        }
        let view = DocumentView::new(&content);
        assert_eq!(view.entry_labels().len(), 0, "{mode}");
        assert!(
            view.lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| !matches!(span.style.fg, Some(theme::MAUVE | theme::BLUE))),
            "{mode}"
        );
    }
}

#[test]
fn list_forms_project_only_the_bound_slice_and_protect_symbolic_names() {
    let mut content = content();
    let mut facts = item(&mut content).entry.take().unwrap();
    let root = EntryInlineRoot::Block { index: 0 };
    facts.kind = EntryKind::Command;
    facts.names = vec!["operator[]".into()];
    facts.forms = vec![EntryForm {
        parts: vec![EntryContentSlice {
            root: root.clone(),
            path: vec![0],
            bytes: Some(0..24),
        }],
    }];
    facts.name_bindings[0].occurrences[0].parts = vec![EntryContentSlice {
        root,
        path: vec![0],
        bytes: Some(0..10),
    }];
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::List {
        kind: ListKind::Bullet,
        compact: true,
        layout: LayoutHint::default(),
        source: None,
        items: vec![ListItem {
            layout: mant_ir::ListItemLayout::default(),
            source: None,
            entry: Some(facts),
            blocks: vec![Block::Paragraph {
                children: vec![text("operator[] [arg=<VALUE>] ordinary [body]=plain")],
                inline_layout: mant_ir::InlineLayout::default(),
                layout: LayoutHint::default(),
                source: None,
            }],
        }],
    }];
    let view = DocumentView::new(&content);
    let rendered = view.render(120);
    let hit = &rendered.search("operator[]")[0];
    let line = &rendered.text.lines[hit.row];
    let visible = line.to_string();
    let start = visible.find("operator[]").unwrap();
    assert_eq!(style_at(line, start + 8).fg, Some(theme::PEACH));
    assert_eq!(style_at(line, start + 11).fg, Some(theme::MAUVE));
    let body = visible.find("[body]").unwrap();
    assert_eq!(style_at(line, body).fg, Some(theme::TEXT));
    assert_eq!(style_at(line, body + 6).fg, Some(theme::TEXT));
}

#[test]
fn composite_forms_keep_pair_state_across_distinct_original_roots() {
    let mut content = content();
    let item = item(&mut content);
    item.terms = vec![
        vec![Inline::Strong {
            children: vec![text("--output"), text("[=")],
        }]
        .into(),
        vec![Inline::Emphasis {
            children: vec![text("<value>]")],
        }]
        .into(),
    ];
    let first = EntryForm::term(0);
    let second = EntryForm::term(1);
    item.entry.as_mut().unwrap().forms = vec![EntryForm {
        parts: first.parts.into_iter().chain(second.parts).collect(),
    }];
    let view = DocumentView::new(&content);
    assert_eq!(view.entry_labels()["output"].text, "--output[=<value>]");
    let rendered = view.render(120);
    for value in ["--output[=", "<value>]"] {
        let hit = &rendered.search(value)[0];
        let line = &rendered.text.lines[hit.row];
        let visible = line.to_string();
        let start = visible.find(value).unwrap();
        for (byte, character) in value.char_indices() {
            if matches!(character, '[' | ']' | '<' | '>') {
                assert_eq!(style_at(line, start + byte).fg, Some(theme::MAUVE));
            }
        }
    }
}

#[test]
fn native_option_terms_retain_the_pinned_reference_text() {
    // The exact input was run through the pinned CVS oracle before this test.
    // man_term.c pre_TP prints the original head; pre_B changes font only;
    // term.c term_word preserves these literal signature markers.
    let source = b".TH SIGNATURE 1\n.SH OPTIONS\n.TP\n.B --output[=<FILE>]\nSelect output.\n.TP\n.B --mode=[fast|slow]\nSelect mode.\n";
    let content = mant_loader::load_roff_bytes(source).unwrap();
    let rendered = DocumentView::new(&content).render(120);
    for label in ["--output[=<FILE>]", "--mode=[fast|slow]"] {
        let hit = &rendered.search(label)[0];
        let line = &rendered.text.lines[hit.row];
        let visible = line.to_string();
        assert_eq!(visible.trim(), label);
        let start = visible.find(label).unwrap();
        assert_eq!(style_at(line, start).fg, Some(theme::GREEN));
        assert_eq!(
            style_at(line, start + label.find('=').unwrap()).fg,
            Some(theme::BLUE)
        );
        assert_eq!(
            style_at(line, start + label.find('[').unwrap()).fg,
            Some(theme::MAUVE)
        );
    }
}

#[test]
fn outline_signature_styles_survive_compact_full_wrapped_selected_and_badged_rows() {
    use crate::navigation::{self, TreePlan};
    let view = DocumentView::new(&content());
    let index = view
        .navigation()
        .iter()
        .position(|node| node.id == "output")
        .unwrap();
    let tree = TreePlan::new(view.navigation()).with_entry_labels(view.entry_labels());
    for width in [1, 8, 16, 28, 120] {
        for (selected, full) in [(usize::MAX, false), (usize::MAX, true), (index, false)] {
            for badges in [
                HashMap::new(),
                HashMap::from([("output".into(), "↗ related=[value]".into())]),
            ] {
                let rows = navigation::rows_with_references(
                    &tree,
                    &[index],
                    selected,
                    &std::collections::HashSet::new(),
                    full,
                    width,
                    &badges,
                );
                assert!(rows.iter().all(|row| row.line.width() <= width));
                for row in &rows {
                    for span in row.line.spans.iter().skip(1) {
                        if span.style.fg == Some(theme::LINK) {
                            continue;
                        }
                        if selected == index && !span.content.trim().is_empty() {
                            assert_eq!(span.style.fg, Some(theme::SELECTED_TEXT));
                            assert_eq!(span.style.bg, Some(theme::SELECTED));
                        } else if full {
                            for character in span.content.chars() {
                                if matches!(character, '[' | ']' | '<' | '>' | '|') {
                                    assert_eq!(span.style.fg, Some(theme::MAUVE));
                                }
                                if character == '=' {
                                    assert_eq!(span.style.fg, Some(theme::BLUE));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let rows = navigation::rows_with_references(
        &tree,
        &[index],
        usize::MAX,
        &std::collections::HashSet::new(),
        false,
        120,
        &HashMap::new(),
    );
    assert!(rows[0].line.to_string().contains("--output"));
    assert!(!rows[0].line.to_string().contains("文件"));
    // An unnamed form used as the compact label retains its original suffix
    // accents even after the matching opening delimiter is truncated away.
    let mut nodes = view.navigation().to_vec();
    nodes[index].title = LABEL.into();
    nodes[index].full_title = None;
    let tree = TreePlan::new(&nodes).with_entry_labels(view.entry_labels());
    let rows = navigation::rows_with_references(
        &tree,
        &[index],
        usize::MAX,
        &std::collections::HashSet::new(),
        false,
        23,
        &HashMap::new(),
    );
    assert!(rows[0].line.to_string().contains("..."));
    let closing = rows[0]
        .line
        .spans
        .iter()
        .find(|span| span.content.contains(']'))
        .unwrap();
    assert_eq!(closing.style.fg, Some(theme::MAUVE));
}

#[test]
fn outline_search_layers_matches_over_cached_signature_accents() {
    use crate::navigation::{
        TreePlan,
        search::{self, OutlineLabels, OutlinePresentation},
    };
    let view = DocumentView::new(&content());
    let index = view
        .navigation()
        .iter()
        .position(|node| node.id == "output")
        .unwrap();
    let tree = TreePlan::new(view.navigation()).with_entry_labels(view.entry_labels());
    let (records, _) = view.outline_search_records();
    let (hits, limited) = search::find(&records, "=");
    assert!(!limited);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].node_index, index);
    for active in [0, usize::MAX] {
        for (full, selected) in [(false, usize::MAX), (true, usize::MAX), (true, index)] {
            let presentation = OutlinePresentation::new(&hits, active);
            let rows = presentation.rows(
                &tree,
                &[index],
                OutlineLabels { selected, full },
                &std::collections::HashSet::new(),
                80,
                &HashMap::new(),
            );
            let equals = rows
                .iter()
                .flat_map(|row| &row.line.spans)
                .find(|span| {
                    span.content.contains('=')
                        && span.style.bg
                            == Some(if active == 0 {
                                theme::SEARCH_ACTIVE
                            } else {
                                theme::SEARCH_MATCH
                            })
                })
                .unwrap();
            assert_eq!(
                equals.style.fg,
                Some(if active == 0 {
                    theme::BASE
                } else if selected == index {
                    theme::SELECTED_TEXT
                } else {
                    theme::BLUE
                })
            );
            if selected != index {
                let opening = rows
                    .iter()
                    .flat_map(|row| &row.line.spans)
                    .find(|span| span.content.contains('['))
                    .unwrap();
                assert_eq!(opening.style.fg, Some(theme::MAUVE));
            }
        }
    }
}
