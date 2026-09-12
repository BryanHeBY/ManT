use libmandoc_rs::{Node, NodeFlags, NodeKind};

use super::{
    horizontal_distance_columns, layout, layout_with_spacing, paragraph_distance_lines,
    vertical_distance_lines,
};
use mant_ir::Block;

fn node(kind: NodeKind, text: Option<&str>, offset: Option<&str>) -> Node {
    Node {
        kind,
        macro_name: None,
        text: text.map(ToOwned::to_owned),
        tag: None,
        line: 0,
        column: 0,
        flow_epoch: 0,
        table_escape: None,
        flags: NodeFlags::default(),
        list_kind: None,
        definition_list_style: None,
        display_kind: None,
        font: None,
        author_mode: None,
        enclosure: None,
        compact: false,
        offset: offset.map(ToOwned::to_owned),
        width: None,
        table_cells: Vec::new(),
        equation: None,
        children: Vec::new(),
    }
}

#[test]
fn converts_mandoc_vertical_units_to_terminal_rows() {
    let empty = node(NodeKind::Root, None, None);
    assert_eq!(paragraph_distance_lines(&empty), Some(1));
    assert_eq!(
        vertical_distance_lines(&node(NodeKind::Text, Some("2v"), None)),
        Some(2)
    );
    assert_eq!(
        vertical_distance_lines(&node(NodeKind::Text, Some("1i"), None)),
        Some(6)
    );
    assert_eq!(
        vertical_distance_lines(&node(NodeKind::Text, Some("not-a-number"), None)),
        None
    );
}

#[test]
fn normalizes_layout_hints() {
    assert_eq!(layout(3.into()).indent_columns, 3);
}

#[test]
fn independent_paragraph_and_vertical_space_requests_are_not_erased() {
    let mut blocks = [
        Block::VerticalSpace {
            lines: 1,
            source: None,
        },
        Block::Paragraph {
            children: Vec::new(),
            layout: layout_with_spacing(4.into(), 1),
            source: None,
        },
    ];

    super::set_block_spacing(&mut blocks[0], 2);

    let Block::Paragraph { layout, .. } = &blocks[1] else {
        panic!("expected paragraph after explicit vertical space");
    };
    assert_eq!(layout.indent_columns, 4);
    assert_eq!(layout.spacing_before_lines, 1);
    assert!(matches!(blocks[0], Block::VerticalSpace { lines: 3, .. }));
}

#[test]
fn converts_horizontal_roff_widths_to_terminal_columns() {
    assert_eq!(horizontal_distance_columns("20"), Some(20));
    assert_eq!(horizontal_distance_columns("8n"), Some(8));
    assert_eq!(horizontal_distance_columns("1i"), Some(10));
    assert_eq!(horizontal_distance_columns("24u"), Some(1));
    assert_eq!(horizontal_distance_columns("+2n"), None);
    assert_eq!(horizontal_distance_columns("wide"), None);
}
