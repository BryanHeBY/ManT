//! Native field/definition ownership stays independent from code-row reading.
use crate::{Block, SOURCES, body_rows, literal_texts, plain_rows, restored};
use mant_ir::ResolvedContent;
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery};
use mant_ui::DocumentView;

fn load(file: &str) -> ResolvedContent {
    let source = std::fs::read(format!("{SOURCES}/seams/{file}.1")).unwrap();
    restored(&mant_loader::load_roff_bytes(&source).unwrap())
}

#[test]
fn literal_hanging_head_still_owns_its_definition_and_exact_name_positions() {
    // All seams sources ran pristine ASCII/UTF-8/tree/lint first. The HP
    // BODY really contains an empty TEXT and a B node; its next IP HEAD is
    // actually empty. man_term.c::print_man_node and term_vspace retain the
    // native row, while this policy rebases the matched native owner proof.
    let content = load("hp-literal-headless-ip");
    let document = content.document.as_ref().unwrap();
    let Block::List { items, .. } = &document.sections[0].blocks[1] else {
        panic!("a real HP/IP owner must still be recognized: {document:#?}")
    };
    let owner = &items[0];
    let entry = owner.entry.as_ref().unwrap();
    assert_eq!(entry.names, ["--one"]);
    assert_eq!(entry.name_bindings.len(), 1);
    let occurrence = &entry.name_bindings[0].occurrences[0];
    assert_eq!(occurrence.parts.len(), 1);
    let part = &occurrence.parts[0];
    let root = mant_ir::EntryOwner::List(owner)
        .inline_root(&part.root)
        .unwrap();
    let path = part
        .path
        .iter()
        .map(|&index| u32::try_from(index).unwrap())
        .collect::<Vec<_>>();
    let leaf = mant_ir::resolve_inline_path(root, &path).unwrap();
    let leaf_text = mant_ir::inline_plain_text(leaf);
    assert_eq!(&leaf_text[part.bytes.clone().unwrap()], "--one");
    assert_eq!(
        owner.source.unwrap().line,
        6,
        "original source, not display row"
    );
    assert_eq!(literal_texts(&content), ["--one", "Description."]);
    let explanation = mant_query::explain_query(
        &content,
        &ExplanationQuery {
            entry: "--one".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let direct = explanation
        .evidence
        .iter()
        .find(|evidence| evidence.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(direct.source, owner.source);
    assert_eq!(direct.entry.as_ref().unwrap().names, ["--one"]);
    let index = content.semantic_index().unwrap();
    assert_eq!(index.section("description").len(), 1);
    for width in [40, 120] {
        let rendered = DocumentView::new(&content).render(width);
        let names = rendered.search("--one");
        assert_eq!(names.len(), 1);
    }
}

#[test]
fn independent_empty_native_rows_and_shared_definition_seams_do_not_mix() {
    // Empty TEXT and NBRZW/BREAK-backed rows are native facts; the approved
    // reading exception is applied only to a completed independent literal.
    for file in ["nf-zero-width-row", "nf-word-break-row"] {
        let content = load(file);
        assert_eq!(literal_texts(&content), ["BODY"]);
        assert_eq!(body_rows(&plain_rows(&content)), ["BODY"]);
    }
    let content = load("inset-shared-literal");
    // Native NODE_LINE on BODY closes HEAD. The BODY itself has no extra
    // empty row to trim, and remains separate rather than joining HEADBODY.
    assert_eq!(body_rows(&plain_rows(&content)), ["HEAD", "BODY"]);
    assert_eq!(literal_texts(&content), ["BODY"]);
}

#[test]
fn initial_row_normalization_rebases_temporary_origins_without_touching_table_text() {
    // CVS roff_term_pre_ti changes the next printed origin; it does not
    // convert the paragraph boundary before nf into a code-content row.
    let content = load("nf-independent-row-origins");
    assert_eq!(literal_texts(&content), ["CODE\nSHIFTED\nPLAIN"]);
    assert_eq!(
        body_rows(&plain_rows(&content)),
        ["", "CODE", "   SHIFTED", "PLAIN"]
    );
    for width in [40, 120] {
        let rendered = DocumentView::new(&content).render(width);
        let code = &rendered.search("CODE")[0];
        let shifted = &rendered.search("SHIFTED")[0];
        let plain = &rendered.search("PLAIN")[0];
        assert_eq!(shifted.row, code.row + 1);
        assert_eq!(plain.row, code.row + 2);
        assert_eq!(shifted.start_column, code.start_column + 3);
        assert_eq!(plain.start_column, code.start_column);
    }
    // Native T{ stores one space-prefixed tbl field, not an empty TEXT row.
    let table = load("tbl-literal-cell");
    let document = table.document.as_ref().unwrap();
    let Block::Table { rows, .. } = &document.sections[0].blocks[1] else {
        panic!("native table remains a table")
    };
    let Block::Paragraph { children, .. } = &rows[0].cells[0].blocks[0] else {
        panic!("simple native table text")
    };
    assert_eq!(mant_ir::inline_plain_text(children), " CODE");
}

#[test]
fn first_pass_literal_motion_positions_the_row_without_copyable_padding() {
    // These exact sources ran against the registered pristine ASCII oracle.
    // term.c's ESCAPE_HORIZ buffers NBRSP; term_field releases the advance
    // before the first ink on the first pass as well as on later passes.
    let mdoc = |body: &str| {
        format!(".Dd January 1, 2024\n.Dt TEST 1\n.Os\n.Sh TEST\n.Bd -literal\nLEFT\n{body}\n.Ed\n")
    };
    let man = |body: &str| format!(".TH TEST 1\n.SH TEST\n.nf\nLEFT\n{body}\n.fi\n");
    for (source, offset) in [
        (mdoc("\\h'|1n'\\c\nRIGHT"), 1),
        (mdoc("\\h'|1n'RIGHT"), 1),
        (man("\\h'|1n'RIGHT"), 1),
        (mdoc("\\h'|0n'RIGHT"), 0),
    ] {
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = restored(&loaded);
        for content in [&loaded, &content] {
            let document = content.document.as_ref().unwrap();
            let Block::Preformatted {
                children,
                inline_layout,
                ..
            } = &document.sections[0].blocks[0]
            else {
                panic!("expected one literal owner: {document:#?}");
            };
            assert_eq!(mant_ir::inline_plain_text(children), "LEFT\nRIGHT");
            let left = inline_layout.row_layout(0).resolve(0, 0);
            let right = inline_layout.row_layout(1).resolve(0, 0);
            assert_eq!(right.first_visual_origin - left.first_visual_origin, offset);
            let plain = plain_rows(content);
            let left = plain.iter().find(|row| row.ends_with("LEFT")).unwrap();
            let right = plain.iter().find(|row| row.ends_with("RIGHT")).unwrap();
            assert_eq!(
                right.len() - "RIGHT".len(),
                left.len() - "LEFT".len() + offset as usize
            );
            for width in [40, 120] {
                let rendered = DocumentView::new(content).render(width);
                let left = &rendered.search("LEFT")[0];
                let right = &rendered.search("RIGHT")[0];
                assert_eq!(right.row, left.row + 1);
                assert_eq!(right.start_column, left.start_column + offset as usize);
            }
        }
    }
}
