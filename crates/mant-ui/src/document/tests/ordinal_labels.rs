//! Numeric-looking definition labels retain their equation and link carriers.

use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

#[test]
fn native_ordinal_payloads_keep_readable_cells_and_clickable_labels() {
    // Fixed CVS mdoc_it_pre() retains both labels in their dt owners. The
    // first contains the MathML equation and the second has href="#2".
    let source = concat!(
        ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
        ".Nm probe\n.Nd exercise descriptors\n.Sh DESCRIPTION\n",
        ".Bl -tag -width Ds\n.It Xo\n.EQ\n(1)\n.EN\n.Xc\nBODY-1\n",
        ".It Sx 2 .\nBODY-2\n.It 3.\nBODY-3\n.El\n",
        ".Sh 1\nDEST-1\n.Sh 2\nDEST-2\n.Sh 3\nDEST-3\n"
    );
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let decoded: ResolvedContent = decoded.into();
    let view = DocumentView::new(&decoded);
    for width in [24, 44, 82, 24] {
        let rendered = view.render(width);
        let hits = rendered.search("( 1 )");
        let [equation] = hits.as_slice() else {
            panic!("readable equation at width {width}");
        };
        let [link] = rendered.links.as_slice() else {
            panic!("section reference at width {width}");
        };
        let region = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
        let mut cells = Buffer::empty(region);
        Paragraph::new(rendered.text.clone()).render(region, &mut cells);
        let symbol = |row: usize, column: usize| {
            cells[(column.try_into().unwrap(), row.try_into().unwrap())].symbol()
        };
        assert_eq!(symbol(equation.row, equation.start_column), "(");
        assert_eq!(symbol(equation.row, equation.end_column - 1), ")");
        assert_eq!(symbol(link.row, link.start_column), "2");
        assert_eq!(link.end_column, link.start_column + 1);
        assert_eq!(
            rendered.link_target_at(link.row, link.start_column),
            Some(&link.target)
        );
        let LinkTarget::Section(id) = &link.target else {
            panic!("original local section target");
        };
        let landing = rendered.anchor_row(id).unwrap();
        assert!(landing < rendered.search("DEST-2")[0].row);
        assert!(landing > rendered.search("DEST-1")[0].row);
        for (row, start, end, word) in [
            (
                equation.row,
                equation.start_column,
                equation.end_column,
                "( 1 )",
            ),
            (link.row, link.start_column, link.end_column, "2"),
        ] {
            assert_eq!(
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition { row, column: start },
                    focus: TextPosition {
                        row,
                        column: end - 1
                    },
                }),
                word
            );
        }
        let first = rendered.search("BODY-1")[0].row;
        let second = rendered.search("BODY-2")[0].row;
        let third = rendered.search("BODY-3")[0].row;
        assert!(equation.row <= first && first < link.row && link.row <= second && second < third);
    }
}
