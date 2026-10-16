use crate::table_matrix::*;
use mant_codec::encode::{MarkdownNode, render_addressable_markdown};
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};

#[path = "../support/reader_actions.rs"]
mod reader_actions;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

fn owner_cell() -> Value {
    json!({"blocks":[{"type":"definition-list","compact":true,"items":[{
        "entry":{"id":"flag-owner","kind":{"kind":"term"},"case":"sensitive",
            "names":["FlagName"],
            "forms":[{"parts":[{"root":{"kind":"term","index":0},"path":[1,0]}]}],
            "nameBindings":[{"name":0,"evidence":"declared","occurrences":[{"parts":[{"root":{"kind":"term","index":0},"path":[1,0],"bytes":{"start":0,"end":8}}]}]}]},
        "terms":[{"content":[{"type":"anchor","id":"flag-owner"},{"type":"strong","children":[{"type":"text","value":"FlagName"}]}]}],
        "headBodyRelation":{"type":"shared","wordBoundary":"separated"},
        "description":[{"type":"paragraph","children":[{"type":"anchor","id":"cell-target"},
            {"type":"link","target":{"kind":"external","uri":"https://example.test/table"},
            "children":[{"type":"emphasis","children":[{"type":"text","value":"CLICK"}]}]}]}]
    }]}]})
}

#[test]
fn framed_owners_keep_pointer_copy_search_and_artifact_ranges_after_resize() {
    for stroke in ["single", "double"] {
        for sparse in [false, true] {
            let mut table = json!({"type":"table","alignment":"center",
                "borders":{"outer":stroke,"innerColumns":stroke,"innerRows":stroke},
                "rows":[{"cells":[owner_cell(),cell("NEIGHBOR")]},
                    {"cells":[cell("SECOND"),cell("LAST")]}]});
            if sparse {
                table["columnCount"] = json!(257);
            }
            let value = restored(&query(table));
            assert_eq!(
                mant_render::render_query_json(&value, true)
                    .unwrap()
                    .matches("https://example.test/table")
                    .count(),
                1
            );
            let artifact = render_addressable_markdown(&value);
            let ranges=artifact.nodes().iter().filter(|node|matches!(node.node(),MarkdownNode::DocumentEntry{names,..} if names.len()==1 && names[0]=="FlagName")).collect::<Vec<_>>();
            assert_eq!(ranges.len(), 1, "one original owner, not a border owner");
            let owned = artifact.text().get(ranges[0].range()).unwrap();
            assert!(
                owned.contains("FlagName") && owned.contains("CLICK"),
                "{owned}"
            );
            assert_eq!(artifact.text().matches("id=\"flag-owner\"").count(), 1);
            let view = DocumentView::new(&value);
            for width in [40_u16, 120, 40] {
                let rendered = view.render(width);
                let hit = &rendered.search("CLICK")[0];
                assert_eq!(rendered.anchor_row("cell-target"), Some(hit.row));
                assert_eq!(rendered.search("FlagName").len(), 1);
                assert_buffer(&rendered, width, &["CLICK", "FlagName"]);
                assert_no_decoration_search(&rendered);
            }
            assert_pointer_copy(&value);
        }
    }
}

fn assert_pointer_copy(value: &mant_ir::ResolvedContent) {
    let mut app = App::new(value);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    for width in [120, 40, 120] {
        terminal.backend_mut().resize(width, 40);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = positions(terminal.backend().buffer(), "CLICK")
            .into_iter()
            .max_by_key(|(x, _)| *x)
            .expect("body link visible in actual viewport");
        click(&mut app, column, row);
        assert_eq!(opened_targets(&mut app), ["https://example.test/table"]);
        select_span(&mut app, column, row, column + 4, None);
        assert_eq!(
            copied_selections(&mut app, |request| match request {
                CopyRequest::Selection { text } => text,
                _ => panic!("visual copy must retain the original label"),
            }),
            ["CLICK"]
        );
    }
}
