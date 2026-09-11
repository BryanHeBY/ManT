//! Tests for the Debian `groff_me(7)` gzip fixture.

use crate::common::{self, DEBIAN_GROFF_ME_SECTIONS};
use crate::fixtures::debian_manual;
use mant_ir::Inline;

#[test]
fn no_argument_request_cells_are_empty_not_literal_zero_width_escapes() {
    let document = debian_manual("groff_me");
    let expected = [
        ("br", "break output line"),
        ("fi", "enable filling"),
        ("na", "disable adjustment of text"),
        ("nf", "disable filling"),
        ("nh", "disable automatic hyphenation"),
        ("ns", "begin no-space mode"),
        ("rs", "resume spacing (end no-space mode)"),
    ];
    let mut found = Vec::new();
    for block in common::document_blocks(document) {
        let mant_ir::Block::Table { rows, .. } = block else {
            continue;
        };
        if rows
            .first()
            .and_then(|row| row.cells.first())
            .is_none_or(|cell| common::block_slice_text(&cell.blocks) != "ad")
        {
            continue;
        }
        for row in rows {
            let name = common::block_slice_text(&row.cells[0].blocks);
            let Some((_, description)) = expected.iter().find(|(key, _)| *key == name) else {
                continue;
            };
            assert_eq!(row.cells.len(), 3, "{name}");
            assert!(
                common::block_slice_text(&row.cells[1].blocks).is_empty(),
                "{name}"
            );
            assert_eq!(common::block_slice_text(&row.cells[2].blocks), *description);
            found.push(name);
        }
    }
    assert_eq!(found, expected.map(|(name, _)| name));
    assert!(
        !document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.unexpanded-table-cell")
        })
    );
}

/// 6-section topology (Name, Synopsis, Description, Files, Notes, See also).
#[test]
fn keeps_complete_section_topology() {
    let document = debian_manual("groff_me");
    common::assert_section_topology("debian/groff_me", document, DEBIAN_GROFF_ME_SECTIONS);
}

/// No roff escapes leak into inline text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("debian/groff_me", debian_manual("groff_me"));
}

/// No duplicate vertical spacing.
#[test]
fn does_not_have_duplicate_vertical_spacing() {
    common::assert_bounded_vertical_spacing(&debian_manual("groff_me").sections, "debian/groff_me");
}

#[test]
fn keeps_mr_fallbacks_as_typed_manual_references() {
    let document = debian_manual("groff_me");
    let mut references = Vec::new();
    common::visit_document_inlines(document, &mut |inline| {
        if let Inline::Link {
            target:
                mant_ir::LinkTarget::Manual {
                    name,
                    manual_section: Some(manual_section),
                },
            ..
        } = inline
        {
            references.push((name.clone(), manual_section.clone()));
        }
    });

    assert_eq!(references.len(), 11);
    for target in [("groff", "7"), ("eqn", "1"), ("troff", "1")] {
        assert!(
            references
                .iter()
                .any(|reference| reference.0 == target.0 && reference.1 == target.1),
            "missing .MR target {target:?}"
        );
    }
}
