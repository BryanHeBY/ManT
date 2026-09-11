//! Owned table/column payloads and normalized equations.

use super::*;

#[test]
fn parser_preserves_infix_eqn_operators() {
    let report = Parser::default()
        .parse_bytes(
            "equation.3",
            b".TH EQUATION 3\n.SH DESCRIPTION\n.EQ\nx + {width over 2}\ny sub 1 sup 2\n.EN\n",
        )
        .expect("parse infix eqn operators");
    let equation = find_kind(&report.document.root, NodeKind::Equation)
        .and_then(|node| node.equation.as_deref())
        .expect("normalized equation");

    assert!(equation.contains("width / 2"), "{equation}");
    assert!(equation.contains("y _ 1 ^ 2"), "{equation}");
}

#[test]
fn parser_normalizes_the_common_gnu_ldots_equation_macro() {
    let report = Parser::default()
        .parse_bytes(
            "equation-ldots.3",
            b".TH EQUATION 3\n.SH DESCRIPTION\n.EQ\nx sub 1 ldots x sub n\n.EN\n",
        )
        .expect("parse GNU ldots equation macro");
    let equation = find_kind(&report.document.root, NodeKind::Equation)
        .and_then(|node| node.equation.as_deref())
        .expect("normalized equation");

    assert_eq!(equation, "x _ 1 ... x _ n");
}

#[test]
fn parser_marks_tbl_text_block_cells() {
    let path = source_path("tbl-text-block");
    fs::write(
        &path,
        ".Dd August 19, 2026\n.Dt TBL-TEXT-BLOCK 3\n.Os\n.Sh NAME\n.Nm demo\n.Nd demo\n.Sh ATTRIBUTES\n.TS\nallbox;\nl l.\nInterface\tValue\nT{\n.Nm\nT}\tMT-Safe\n.TE\n",
    )
    .expect("write tbl text block source");
    let document = parse_file(&path, false).expect("parse tbl text block source");
    fs::remove_file(path).expect("remove tbl text block source");
    let row = find_node(&document.root, &|node| {
        node.kind == NodeKind::Table && node.table_cells.iter().any(|cell| cell.text_block)
    })
    .expect("tbl row containing a text block");
    assert_eq!(row.table_cells.len(), 2);
    assert_eq!(row.table_cells[0].text.as_deref(), Some(""));
    assert!(row.table_cells[0].text_block);
    assert!(!row.table_cells[1].text_block);
}

#[test]
fn parser_retains_the_parse_time_value_of_tbl_text_block_strings() {
    let document = Parser::default()
        .parse_bytes(
            "tbl-expanded-string.1",
            b".TH TBL-EXPANDED-STRING 1\n.ds Aq \\(aq\n.SH DESCRIPTION\n.TS\nl.\nT{\nThere\\*(Aqs\nT}\n.TE\n",
        )
        .expect("parse tbl source with a string expansion")
        .document;
    let row = find_node(&document.root, &|node| {
        node.kind == NodeKind::Table && node.table_cells.iter().any(|cell| cell.text_block)
    })
    .expect("tbl row containing a text block");
    assert!(row.table_cells[0].text_block);
    // roff.c expands the string before tbl_read() persists tbl_dat::string,
    // while retaining the resulting named-character escape for later output.
    // This is native evaluated content, not source spelling that a later
    // parser may safely reinterpret without the original string table.
    assert_eq!(row.table_cells[0].text.as_deref(), Some(r"There\(aqs"));
}

#[test]
fn parser_marks_both_tbl_vertical_continuation_forms() {
    let document = Parser::default()
        .parse_bytes(
            "tbl-vertical-continuations.1",
            b".TH TBL-VERTICAL-CONTINUATIONS 1\n.SH TABLES\n.TS\nl l.\nfirst\tvalue\n\\^\tcontinued\n.TE\n.TS\nl l,\n^ l.\nfirst\tvalue\n\tcontinued\n.TE\n",
        )
        .expect("parse tbl vertical continuations")
        .document;

    let explicit = find_node(&document.root, &|node| {
        node.kind == NodeKind::Table && node.line == 6
    })
    .expect("explicit continuation row");
    assert!(explicit.table_cells[0].vertical_continuation);

    let layout = find_node(&document.root, &|node| {
        node.kind == NodeKind::Table && node.line == 12
    })
    .expect("layout continuation row");
    assert!(layout.table_cells[0].vertical_continuation);
}

#[test]
fn parser_retains_column_list_cells() {
    let report = Parser::default()
        .parse_bytes(
            "columns.3",
            b".Dd August 19, 2026\n.Dt COLUMNS 3\n.Os\n.Sh DESCRIPTION\n\
.Bl -column name type description\n.It Dv CLSET_TIMEOUT Ta \"struct timeval *\" Ta \"set total timeout\"\n.El\n",
        )
        .expect("parse mdoc column list");
    let item = find_macro(&report.document.root, "It").expect("column item");
    let bodies = item
        .children
        .iter()
        .filter(|child| child.kind == NodeKind::Body)
        .collect::<Vec<_>>();

    assert_eq!(bodies.len(), 3);
    assert_eq!(
        bodies
            .iter()
            .map(|body| {
                body.children
                    .iter()
                    .flat_map(|child| child.children.iter())
                    .chain(body.children.iter())
                    .filter_map(|child| child.text.as_deref())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [
            vec!["CLSET_TIMEOUT"],
            vec!["struct timeval *"],
            vec!["set total timeout"],
        ]
    );
}

#[test]
fn parser_copies_table_cells_and_equation_text() {
    let path = source_path("structured-payload-mandoc-session");
    fs::write(
        &path,
        ".TH PAYLOAD 1\n.SH TABLE\n.TS\ntab(|);\nl r.\nleft|right\n.TE\n\
         .SH EQUATION\n.EQ\nx sup 2\n.EN\n",
    )
    .expect("write table and equation source");

    let document = parse_file(&path, false).expect("parse table and equation source");
    fs::remove_file(path).expect("remove table and equation source");

    let table = find_kind(&document.root, NodeKind::Table).expect("table row node");
    assert_eq!(table.table_cells.len(), 2);
    assert_eq!(table.table_cells[0].text.as_deref(), Some("left"));
    assert_eq!(table.table_cells[1].alignment, TableAlignment::Right);
    let equation = find_kind(&document.root, NodeKind::Equation).expect("equation node");
    assert!(
        equation
            .equation
            .as_deref()
            .is_some_and(|value| value.contains('x'))
    );
}
