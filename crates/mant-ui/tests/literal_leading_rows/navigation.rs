//! Scalar provenance and actual reader links survive the omitted initial row.
use crate::{Inline, ResolvedContent, cases, load, restored};
use mant_ir::visit::Visit;
use mant_ui::DocumentView;

#[test]
fn anchors_and_link_labels_move_with_their_original_literal_glyphs() {
    for case in cases() {
        let id = case["id"].as_str().unwrap();
        if !matches!(
            id,
            "bd_anchor_after_leading_row" | "ex_link_after_leading_row"
        ) {
            continue;
        }
        let content = restored(&load(&case));
        let mut links = Links(vec![]);
        links.visit_document(content.document.as_ref().unwrap());
        if id == "ex_link_after_leading_row" {
            assert_eq!(links.0.len(), 1);
            assert_eq!(
                links.0[0],
                (
                    mant_ir::LinkTarget::External {
                        uri: "https://example.test/literal-rows".into()
                    },
                    "LINE_A".into()
                )
            );
        }
        for width in [80, 120] {
            let rendered = DocumentView::new(&content).render(width);
            let first = rendered.search("LINE_A");
            let second = rendered.search("LINE_B");
            assert_eq!(first.len(), 1);
            assert_eq!(second.len(), 1);
            assert_eq!(second[0].row, first[0].row + 1);
            if id == "bd_anchor_after_leading_row" {
                assert_eq!(rendered.anchor_row("literal-anchor"), Some(first[0].row));
            }
        }
        assert_search_range(&content);
    }
}

struct Links(Vec<(mant_ir::LinkTarget, String)>);
impl<'ir> Visit<'ir> for Links {
    fn visit_inline(&mut self, node: &'ir Inline) {
        if let Inline::Link {
            target, children, ..
        } = node
        {
            self.0
                .push((target.clone(), mant_ir::inline_plain_text(children)));
        }
        mant_ir::visit::walk_inline(self, node);
    }
}

fn assert_search_range(content: &ResolvedContent) {
    use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};
    let artifact = mant_codec::encode::render_addressable_markdown(content);
    let found = mant_query::search_query(
        content,
        &SearchQuery {
            pattern: "LINE_A".into(),
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
    assert_eq!(found.total, 1);
    assert_eq!(found.matches.len(), 1);
    for hit in &found.matches {
        assert_eq!(hit.occurrences.len(), 1);
        for occurrence in &hit.occurrences {
            assert_eq!(occurrence.matched_text, "LINE_A");
            let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
            let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
            assert_eq!(&artifact.text()[start..end], "LINE_A");
        }
    }
}

#[path = "../support/reader_actions.rs"]
mod reader_actions;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

#[test]
fn pointer_hits_and_copy_use_the_rebased_code_label_and_authored_whitespace() {
    use mant_ui::{App, CopyRequest};
    use ratatui::{Terminal, backend::TestBackend};
    for (id, copied, columns) in [
        ("ex_link_after_leading_row", "LINE_A", 6),
        ("nf_authored_spaces", "  ", 2),
        ("ex_authored_tab", "     ", 8),
        ("bd_authored_nbsp", "\u{a0}", 2),
    ] {
        let case = cases().into_iter().find(|case| case["id"] == id).unwrap();
        let content = restored(&load(&case));
        let mut app = App::new(&content);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = positions(terminal.backend().buffer(), "LINE_A")
            .into_iter()
            .max_by_key(|(column, _)| *column)
            .unwrap();
        let selected_row = if id == "ex_link_after_leading_row" {
            click(&mut app, column, row);
            assert_eq!(
                opened_targets(&mut app),
                ["https://example.test/literal-rows"]
            );
            row
        } else {
            row - 1
        };
        // A literal tab at the default three-cell document origin occupies
        // five source cells. Selecting eight also proves surface fill is not
        // copied. NBSP likewise owns one cell despite a two-cell selection.
        select_span(&mut app, column, selected_row, column + columns - 1, None);
        assert_eq!(
            copied_selections(&mut app, |request| match request {
                CopyRequest::Selection { text } => text,
                _ => panic!("code selection uses source text"),
            }),
            [copied],
            "{id}"
        );
    }
}
