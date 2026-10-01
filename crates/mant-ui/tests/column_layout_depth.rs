//! Actual complete nested columns survive the JSON and terminal-cell boundary.

use mant_ui::DocumentView;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd test page\n.Sh DESCRIPTION\n";

#[test]
fn complete_nested_columns_keep_every_cell_in_wide_and_narrow_buffers() {
    // Exact complete sources (0/1/2/4/8/12/16) ran pristine profiles first,
    // with no lint or structure loss. termp_it_pre owns each column field.
    for depth in [0, 1, 2, 4, 8, 12, 16] {
        let source = format!(
            "{HEADER}{}leaf\n{}",
            ".Bl -column \"xx\"\n.It head\n".repeat(depth),
            ".El\n".repeat(depth)
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let view = DocumentView::new(&decoded.into());
        for width in [20, 40, 78, 120, 40] {
            let rendered = view.render(width);
            let heads = rendered.search("head");
            assert_eq!(heads.len(), depth, "depth={depth}, width={width}");
            let leaves = rendered.search("leaf");
            assert_eq!(leaves.len(), 1);
            let area = Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
            let mut buffer = Buffer::empty(area);
            rendered.text.clone().render(area, &mut buffer);
            // Search coordinates identify the actual terminal cells, not
            // another independently reconstructed text projection.
            for hit in heads.iter().chain(&leaves) {
                let column = u16::try_from(hit.start_column).unwrap();
                let row = u16::try_from(hit.row).unwrap();
                assert!(matches!(buffer[(column, row)].symbol(), "h" | "l"));
            }
            assert!(!rendered.text.to_string().contains('\u{fffd}'));
        }
    }
}
