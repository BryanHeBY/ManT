//! Native post consumption and IR-owner retirement have different lifetimes.

use super::*;

const MDOC: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

#[test]
fn an_empty_item_post_preserves_the_unbuffered_backafter_register() {
    // Exact source ran the five pristine profiles before this assertion.
    // term_newln()475-481 does not flush an empty buffer: a bare BACKAFTER
    // survives the It BODY post and still consumes BodyWord's first glyph.
    let source = format!("{MDOC}.Bl -ohang\n.It HEAD\n.No \\z\n.El\n.No BodyWord\n");
    let document = parse_manual_bytes(
        std::path::Path::new("empty-item-post-register.1"),
        source.as_bytes(),
    )
    .unwrap();
    let text = projected_document_text(&document);
    assert!(text.ends_with("HEADodyWord"), "{document:#?}");
}

#[test]
fn completed_head_ownership_does_not_hide_later_invisible_synopsis_rows() {
    // These exact five-profile receipts retain the original missing-NAME
    // lint status 2. synopsis_pre() calls term_vspace before the second Ft;
    // term_newln must retire the old HEAD row, not that newly asserted gap.
    let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh SYNOPSIS\n.Bl -tag -width Ds\n.It x\n.Ft \\&\n.Ft \\&\n.sp 0\n.No BODY\n.El\n";
    let document = parse_manual_bytes(
        std::path::Path::new("completed-head-synopsis-rows.1"),
        source,
    )
    .unwrap();
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("{document:#?}");
    };
    let rows = items[0]
        .description
        .iter()
        .filter_map(|block| match block {
            Block::VerticalSpace { lines, .. } => Some(*lines),
            _ => None,
        })
        .sum::<u16>();
    assert_eq!(rows, 2, "{document:#?}");
    assert_eq!(items[0].head_body_relation, HeadBodyRelation::Separate);
}

#[test]
fn man_synopsis_body_uses_the_page_margin_after_its_name_field() {
    // Exact five-profile receipt preceded this test (lint 2: missing NAME).
    // pre_SY(BODY), man_term.c861-864, restores maxrmargin independently
    // of the HEAD field's short bound and each later fi/nf destination.
    let source = b".TH PROBE 1\n.SH TEST\n.SY probe\n.nf\nALPHA\n.fi\nBETA\nSECOND\n.nf\nGAMMA\n.sp 2\nDELTA\n.fi\n.YS\nAFTER\n";
    let document =
        parse_manual_bytes(std::path::Path::new("synopsis-body-page-margin.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert!(text.contains("BETA SECOND"), "{document:#?}");
    assert!(!text.contains("BETA\nSECOND"), "{document:#?}");
}

#[test]
fn a_paragraph_request_consumes_the_old_rejected_field_before_the_next_word() {
    // Exact five-profile receipt was lint-clean. termp_pp_pre() executes
    // term_vspace -> term_newln before its endline (mdoc_term.c1579-1584,
    // term.c489-497). The accepted D survives; E dies with that old field;
    // AFTER belongs to a fresh buffer and cannot be discarded with E.
    let source = format!(
        "{MDOC}.Bl -tag -width 8n\n.It HeadWord\n.No \"D \\p E\"\n.Pp\n.No AFTER\n.El\n.Sh NEXT\n.No END\n"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("paragraph-native-field-retirement.1"),
        source.as_bytes(),
    )
    .unwrap();
    let text = projected_document_text(&document);
    assert!(text.contains("HeadWordDAFTER"), "{document:#?}");
    assert!(!text.contains("HeadWordDE"), "{document:#?}");
}

#[test]
fn a_real_name_pre_retires_only_its_completed_prefix_owner() {
    // Both full sources ran pristine five profiles first (original missing
    // NAME lint 2 preserved). synopsis_pre() ends PREFIX before a real Nm
    // HEAD; html_fillmode/termp_nm_pre keep the declaration's rich semantics
    // under no-fill rather than turning its Strong name into fenced text.
    for no_fill in [false, true] {
        let source = if no_fill {
            ".Dd November 4, 2026\n.Dt PROBE 1\n.Os\n.Sh SYNOPSIS\n.nf\n.Xo\n.No PREFIX\\z\n.Nm name\n.No SUFFIX\n.Xc\n.fi\n.Sh NEXT\nAFTER.\n"
        } else {
            ".Dd November 4, 2026\n.Dt PROBE 1\n.Os\n.Sh SYNOPSIS\n.No PREFIX\n.Nm name\n.No SUFFIX\n.Sh NEXT\nAFTER.\n"
        };
        let document = parse_manual_bytes(
            std::path::Path::new("name-pre-semantic-owner.1"),
            source.as_bytes(),
        )
        .unwrap();
        let paragraphs: Vec<_> = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph {
                    children, source, ..
                } => Some((children, source)),
                _ => None,
            })
            .collect();
        let (declaration, source) = paragraphs.last().unwrap();
        assert_eq!(source.unwrap().line, if no_fill { 8 } else { 6 });
        assert!(
            strong_document_text(&document).contains("name"),
            "{document:#?}"
        );
        assert_eq!(
            inline_text(declaration),
            if no_fill {
                "name\nSUFFIX"
            } else {
                "name SUFFIX"
            },
            "{document:#?}"
        );
        if !no_fill {
            assert_eq!(paragraphs.len(), 2, "{document:#?}");
            assert_eq!(inline_text(paragraphs[0].0), "PREFIX");
        }
    }
}

#[test]
fn each_original_nested_item_head_installs_its_own_native_flags() {
    // Exact source reran ASCII/UTF-8/HTML/tree/lint before this assertion;
    // its original empty tag width warning is retained (lint 1). It HEAD pre
    // installs NOBREAK/BRTRSP/BRIND on the existing tcol, independent of an
    // outer item's already completed HEAD post (mdoc_term.c:782-861).
    let source = format!(
        "{MDOC}.Bl -tag -width 4n\n.It Xo\n.No X\n.Xc\n.Bl -tag -width inner\n.It INNER\n.No BodyWord\n.El\n.No AFTER\n.El\n.fi\n.Sh NEXT\n.No END\n"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("nested-original-head-pre.1"),
        source.as_bytes(),
    )
    .unwrap();
    let [Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("{document:#?}");
    };
    let inner = items[0]
        .description
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(&items[0]),
            _ => None,
        })
        .unwrap();
    assert!(
        matches!(inner.head_body_relation, HeadBodyRelation::Shared { .. }),
        "{document:#?}"
    );
    assert_eq!(inline_text(&inner.terms[0].content), "INNER");
}
