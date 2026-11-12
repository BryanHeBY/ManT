//! Content behavior contracts.

use super::*;
#[test]
fn lowers_man_sections_fonts_definitions_and_literal_blocks() {
    let path = temporary_source(
        "man",
        ".TH MANT 1 \"July 2026\"\n\
         .SH NAME\n\
         mant \\- a viewer\n\
         .SH OPTIONS\n\
         .TP\n\
         \\fB\\-h\\fR\n\
         Show help.\n\
         .nf\n\
         mant --help\n\
         mant git\n\
         .fi\n",
    );

    let document = parse_manual_source(&path).expect("lower man source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(
        document
            .sections
            .iter()
            .map(|section| section.heading.plain_text())
            .collect::<Vec<_>>(),
        vec!["NAME", "OPTIONS"]
    );
    assert!(
        document.sections[1]
            .blocks
            .iter()
            .any(|block| matches!(block, Block::DefinitionList { .. }))
    );
    assert!(document.sections[1].blocks.iter().any(|block| matches!(
        block,
        Block::DefinitionList { items, .. }
            if items.iter().any(|item| item.description.iter().any(
                |description| matches!(description, Block::Preformatted { .. })
            ))
    )));
}

#[test]
fn lowers_mdoc_semantic_inline_nodes_and_nested_sections() {
    let path = temporary_source(
        "mdoc",
        ".Dd July 19, 2026\n\
         .Dt MANT 1\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         Use\n\
         .Nm mant\n\
         with\n\
         .Xr man 1\n\
         Read\n\
         .Lk https://example.test/docs \"the documentation\"\n\
         or contact\n\
         .Mt docs@example.test\n\
         .Ss Details\n\
         .Fl h\n",
    );

    let document = parse_manual_source(&path).expect("lower mdoc source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.source.format, SourceFormat::Mdoc);
    assert_eq!(
        document.sections[0].children[0].heading.plain_text(),
        "Details"
    );
    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected description paragraph");
    };
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
    assert!(
        children.iter().any(
            |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Manual { name, .. }, .. } if name == "man")
        )
    );
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. } if uri == "https://example.test/docs")
    ));
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Email { address }, .. } if address == "docs@example.test")
    ));
}

#[test]
fn root_blocks_receive_the_same_navigation_passes_as_sections() {
    let mdoc = parse_manual_bytes(
        std::path::Path::new("root-section-reference.1"),
        b".Dd September 3, 2026\n.Dt ROOT-SECTION-REFERENCE 1\n.Os\n\
See\n.Sx NAME\n.Sh NAME\n.Nm root-section-reference\n.Nd root reference\n",
    )
    .expect("lower a root-level mdoc section reference");
    assert!(mdoc.blocks.iter().any(|block| {
        matches!(block, Block::Paragraph { children, .. } if children.iter().any(|inline| {
            matches!(inline, Inline::Link {
                target: mant_ir::LinkTarget::Section { id }, ..
            } if id == "name")
        }))
    }));
    assert!(mdoc.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_deref() != Some("ir.dangling-section-link")
            && diagnostic.code.as_deref() != Some("unresolved-section-reference")
    }));

    let man = parse_manual_bytes(
        std::path::Path::new("root-manual-reference.1"),
        b".TH ROOT-MANUAL-REFERENCE 1\n.BR printf (3)\n.SH NAME\nroot-manual-reference \\- root reference\n",
    )
    .expect("lower a root-level traditional manual reference");
    assert!(man.blocks.iter().any(|block| {
        matches!(block, Block::Paragraph { children, .. } if children.iter().any(|inline| {
            matches!(inline, Inline::Link {
                target: mant_ir::LinkTarget::Manual {
                    name,
                    manual_section: Some(section),
                }, ..
            } if name == "printf" && section == "3")
        }))
    }));
}

#[test]
fn tbl_user_macro_expansion_preserves_native_text_without_fabricating_links() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-text-link.1"),
        b".TH TABLE-TEXT-LINK 1\n\
.nr do-fallback 0\n\
.if !\\n(.f .nr do-fallback 1\n\
.if \\n[do-fallback] \\{\\\n\
.  de MR\n\
.    ie \\\\n(.$=1 \\\n\
.      I \\%\\\\$1\n\
.    el \\\n\
.      IR \\%\\\\$1 (\\\\$2)\\\\$3\n\
.  .\n\
.\\}\n\
.rr do-fallback\n\
.de UNUSED\n\
THIS_MACRO_IS_NOT_CALLED\n\
..\n\
.SH DESCRIPTION\n\
.TS\ntab($);\nl l.\ngrn$T{\nrenders\n.MR gremlin 1\ndiagrams;\nT}\n\
gperl$T{\npopulates\n.I groff\nregisters using\n.MR perl 1 ;\nT}\n.TE\n",
    )
    .expect("lower a tbl text block using a document-local macro");

    let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("table content must not escape into a separate paragraph");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[1].blocks.as_slice() else {
        panic!("expected semantic table cell paragraph");
    };
    assert_eq!(inline_text(children), "renders gremlin (1) diagrams;");
    assert!(
        !children
            .iter()
            .any(|child| matches!(child, Inline::Link { .. }))
    );
    let [Block::Paragraph { children, .. }] = rows[1].cells[1].blocks.as_slice() else {
        panic!("expected styled semantic table cell paragraph");
    };
    assert_eq!(
        inline_text(children),
        "populates groff registers using perl (1);"
    );
    assert!(
        !children
            .iter()
            .any(|child| matches!(child, Inline::Link { .. }))
    );
}

#[test]
fn preserves_printable_roff_content_outside_formal_sections() {
    let document = parse_manual_bytes(
        std::path::Path::new("manweb.1"),
        b".TH MANWEB 1\n .SH NAME\nmanweb - browse generated documentation\n.SH SYNOPSIS\n.B manweb\n",
    )
    .expect("lower root prose");
    let [Block::Paragraph { children, .. }] = document.blocks.as_slice() else {
        panic!("expected one root paragraph, got {:?}", document.blocks);
    };

    assert_eq!(
        inline_text(children),
        " .SH NAME manweb - browse generated documentation"
    );
    assert_eq!(document.sections[0].heading.plain_text(), "SYNOPSIS");
}
