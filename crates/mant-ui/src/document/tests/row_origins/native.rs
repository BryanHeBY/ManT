//! Native origins retain source ownership across wire and visual consumers.
use super::super::{
    Block, DocumentView, EntryFacts, ExternalUri, LinkTarget, RenderedDocument, RenderedSelection,
    ResolvedContent, TextPosition,
};

#[test]
fn native_definition_origins_survive_query_json_and_visual_copy() {
    // Exact inputs were run with registered pristine CVS ASCII, UTF-8 and
    // lint before adding this assertion. print_mdoc_node() handles NODE_LINE
    // before saving geometry; roff requests return without restoring offset,
    // while outer non-text scopes restore it before the last buffered word
    // flushes (mdoc_term.c:314-330, 393-397, 437-439).
    for (head, expected) in [
        (
            ".No Beta\n.No Gamma\n.No Delta\n",
            vec![("Alpha", 0), ("Beta", 6), ("Gamma", 6), ("Delta", 0)],
        ),
        (".No Beta\n", vec![("Alpha", 0), ("Beta", 0)]),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo\n.No Alpha\n.nf\n{head}.Xc\n.No BodyWord\n.El\n"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let content: ResolvedContent = decoded.into();
        let plain = mant_render::render_query_text(&content);
        for (word, origin) in &expected {
            let row = plain
                .lines()
                .find(|row| row.trim() == *word)
                .expect("CLI row");
            assert_eq!(row, format!("{}{word}", " ".repeat(*origin)), "{plain}");
        }
        let body = plain.lines().find(|row| row.trim() == "BodyWord").unwrap();
        assert_eq!(body, "      BodyWord", "{plain}");
        let markdown = mant_codec::encode::render_markdown(&content);
        // Both exact sources reran pristine in all five profiles for LC3.
        // Ordinary Markdown keeps the hard rows without turning the native
        // reading origin into NBSP. Native CLI/TUI origins remain strict below.
        let head_rows = expected
            .iter()
            .map(|(word, _)| *word)
            .collect::<Vec<_>>()
            .join("  \n  ");
        assert!(markdown.contains(&format!("- {head_rows}")), "{markdown}");
        assert!(!markdown.contains("&#160;"), "{markdown}");
        let view = DocumentView::new(&content);
        for width in [20, 80, 120] {
            let rendered = view.render(width);
            let area =
                ratatui::layout::Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
            let mut buffer = ratatui::buffer::Buffer::empty(area);
            ratatui::widgets::Widget::render(
                ratatui::widgets::Paragraph::new(rendered.text.clone()),
                area,
                &mut buffer,
            );
            let alpha = &rendered.search("Alpha")[0];
            for (word, origin) in &expected {
                let hit = &rendered.search(word)[0];
                assert_eq!(
                    hit.start_column,
                    alpha.start_column + origin,
                    "{word}, width={width}"
                );
                assert_eq!(
                    buffer[(
                        u16::try_from(hit.start_column).unwrap(),
                        u16::try_from(hit.row).unwrap()
                    )]
                        .symbol(),
                    &word[..1],
                );
                let copy = rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: hit.row,
                        column: 0,
                    },
                    focus: TextPosition {
                        row: hit.row,
                        column: hit.end_column - 1,
                    },
                });
                // Row hints no longer contribute copyable text; the existing
                // structural section origin remains part of visual copying.
                assert_eq!(
                    copy,
                    format!(
                        "{}{word}",
                        " ".repeat(hit.start_column.saturating_sub(*origin))
                    )
                );
            }
        }
    }
}

#[test]
fn first_named_origin_preserves_body_addresses_across_wire_and_visual_consumers() {
    // The exact first_named_origin input ran pristine CVS ASCII, UTF-8 and
    // HTML before these assertions. mdoc_term.c::print_mdoc_node saves and
    // restores a non-text scope's offset; roff_term.c::roff_term_pre_br runs
    // the BRIND transition; term.c::term_fill/term_field decide graph padding.
    // This label has relative origin zero. Only its BODY link has origin six.
    let source = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh OPTIONS\n.Bl -tag -width 4n\n.It Xo\n.br\n.Fl alpha\n.Xc\n.Lk https://example.org BodyWord\n.El\n";
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let content: ResolvedContent = decoded.into();
    let document = content.document.as_ref().unwrap();
    let entries = mant_ir::content_entries(&document.sections[1].blocks);
    let entry = entries
        .iter()
        .find(|entry| entry.names().iter().any(|name| name == "-alpha"))
        .unwrap();
    let owner = entry.owner();
    let facts = owner.facts().unwrap();
    let binding = facts
        .name_bindings
        .iter()
        .find(|binding| facts.names[binding.name] == "-alpha")
        .unwrap();
    assert_eq!(binding.occurrences.len(), 1);
    let occurrence = &binding.occurrences[0];
    assert_eq!(
        mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
        "-alpha"
    );
    // Fl emits the hyphen before its child (mdoc_term.c::termp_fl_pre).
    // One name can therefore span multiple original leaves in the same term.
    let mut next_scalar = 0;
    for part in &occurrence.parts {
        let projected = mant_ir::project_content_slice(owner, part).unwrap();
        assert_eq!(projected.root, mant_ir::EntryInlineRoot::Term { index: 0 });
        assert_eq!(projected.chars.start, next_scalar);
        next_scalar = projected.chars.end;
    }
    assert_eq!(next_scalar, 6);
    let mant_ir::EntryOwner::Definition(item) = owner else {
        panic!("native tag owner");
    };
    assert_eq!(mant_ir::inline_plain_text(&item.terms[0]), "-alpha");
    assert_eq!(item.terms[0].inline_layout.row_indent(0), 0);
    assert_first_named_explanation(&content, &entry.content());
    assert_first_named_artifact(&content, facts);
    let view = DocumentView::new(&content);
    for width in [40, 80, 120] {
        assert_first_named_visual(&view.render(width), width, facts.id.as_str());
    }
}

fn assert_first_named_explanation(content: &ResolvedContent, original: &Block) {
    let explanation = mant_query::select_explanation(content, "-alpha").unwrap();
    let wire = serde_json::to_string(&explanation).unwrap();
    let restored: mant_protocol::QueryExplanation = serde_json::from_str(&wire).unwrap();
    assert_eq!(restored, explanation);
    let evidence = &restored.evidence[0];
    let Some(mant_protocol::ExplanationContent::Entry { block }) = &evidence.content else {
        panic!("complete source owner");
    };
    assert_eq!(block, original);
    let entry = evidence.entry.as_ref().unwrap();
    let binding = entry
        .name_bindings
        .iter()
        .find(|binding| entry.names[binding.name_index as usize] == "-alpha")
        .unwrap();
    assert_eq!(binding.occurrences.len(), 1);
    let mut selected = String::new();
    let mut next_scalar = 0;
    for range in &binding.occurrences[0].content {
        assert!(
            matches!(range, mant_protocol::ExplanationContentRange::DefinitionTerm {
            path, item_index: 0, term_index: 0, ..
        } if path.is_empty())
        );
        assert_eq!(range.char_range().start, next_scalar);
        next_scalar = range.char_range().end;
        let text = range.resolve(block).unwrap().safe_text();
        selected.extend(
            text.chars()
                .skip(range.char_range().start)
                .take(range.char_range().len()),
        );
    }
    assert_eq!(next_scalar, 6);
    assert_eq!(selected, "-alpha");
}

fn assert_first_named_artifact(content: &ResolvedContent, facts: &EntryFacts) {
    let artifact = mant_codec::encode::render_addressable_markdown(content);
    let mapped = artifact
        .nodes()
        .iter()
        .find(|mapped| {
            matches!(
                mapped.node(),
                mant_codec::encode::MarkdownNode::DocumentEntry { names, .. }
                    if names.iter().any(|name| name == "-alpha")
            )
        })
        .unwrap();
    let mant_codec::encode::MarkdownNode::DocumentEntry { owner, .. } = mapped.node() else {
        unreachable!("selected entry");
    };
    assert!(std::ptr::eq(owner.facts().unwrap(), facts));
    let rendered = &artifact.text()[mapped.range()];
    assert!(rendered.contains("-alpha"), "{rendered}");
    assert!(rendered.contains("BodyWord"), "{rendered}");
    assert!(rendered.contains("https://example.org"), "{rendered}");
}

fn assert_first_named_visual(rendered: &RenderedDocument, width: u16, owner_id: &str) {
    let alpha = &rendered.search("-alpha")[0];
    let body = &rendered.search("BodyWord")[0];
    assert_eq!(body.start_column, alpha.start_column + 6);
    assert_eq!(body.row, alpha.row + 1);
    assert_eq!(rendered.anchor_row(owner_id), Some(alpha.row));
    assert_eq!(
        rendered.link_target_at(body.row, body.start_column),
        Some(&LinkTarget::External(
            ExternalUri::parse("https://example.org").unwrap()
        ))
    );
    assert_eq!(
        rendered.link_target_at(body.row, body.start_column - 1),
        None
    );
    let area = ratatui::layout::Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    ratatui::widgets::Widget::render(
        ratatui::widgets::Paragraph::new(rendered.text.clone()),
        area,
        &mut buffer,
    );
    for (hit, word) in [(alpha, "-alpha"), (body, "BodyWord")] {
        assert_eq!(
            buffer[(
                u16::try_from(hit.start_column).unwrap(),
                u16::try_from(hit.row).unwrap()
            )]
                .symbol(),
            &word[..1]
        );
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column,
                },
                focus: TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1,
                },
            }),
            word
        );
    }
}
