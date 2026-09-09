//! Regressions promoted from the final NetBSD and `DragonFly` BSD release audit.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

use common::{block_slice_text, definition_items, inline_text, section};
use fixtures::bsd_manual;

#[test]
fn netbsd_drm_decodes_the_authored_caron_name() {
    let document = bsd_manual("netbsd-drm");
    let authors = block_slice_text(&section(document, "AUTHORS").blocks);

    assert!(authors.contains("Jaromír Doleček"), "authors={authors:?}");
    assert!(!authors.contains(r"\[vc]"), "authors={authors:?}");
}

#[test]
fn dragonfly_adduser_carries_sm_off_into_a_display_line() {
    let document = bsd_manual("dragonfly-adduser");
    let format = block_slice_text(&section(document, "FORMAT").blocks);

    assert!(
        format.contains("name:uid:gid:class:change:expire:gecos:home_dir:shell:password"),
        "format={format:?}"
    );
}

#[test]
fn dragonfly_gdb_preserves_independent_tp_option_declarations() {
    let document = bsd_manual("dragonfly-gdb");
    let options = section(document, "OPTIONS");
    let aliases = definition_items(options)
        .into_iter()
        .map(|item| {
            item.terms
                .iter()
                .map(|term| inline_text(term))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    for form in ["-symbols=file", "-s file", "-exec=file", "-e file"] {
        assert!(
            aliases.contains(&vec![form.into()]),
            "declarations={aliases:?}"
        );
    }
}

#[test]
fn dragonfly_gdb_restriction_bullets_are_not_semantic_terms() {
    fn check(entries: &[mant_ir::SemanticEntry]) {
        for entry in entries {
            assert!(!entry.forms.iter().any(|form| form == "•"));
            check(&entry.children);
        }
    }
    let document = bsd_manual("dragonfly-gdb");
    let index = mant_ir::SemanticIndex::build(document);
    let mut sections = Vec::new();
    common::collect_sections(&document.sections, &mut sections);
    for section in sections {
        check(index.section(&section.id));
    }
    let text = mant_render::render_query_text(&common::query_for_document("gdb", document));
    for body in [
        "Start your program",
        "Make your program stop",
        "Examine what has happened",
        "Change things in your program",
    ] {
        assert!(text.contains(body), "lost {body}");
    }
    assert!(
        section(document, "DESCRIPTION")
            .blocks
            .iter()
            .any(|block| matches!(block,
        mant_ir::Block::List { kind: mant_ir::ListKind::Bullet, items, .. } if items.len() == 4))
    );
}

#[test]
fn openbsd_term_preserves_digits_after_a_signed_legacy_size() {
    let document = bsd_manual("openbsd-current-term");
    let example = block_slice_text(&section(document, "EXAMPLE").blocks);

    assert!(
        example.contains("0000  1a 01 10 00 02 00 03 00  82 00 31 00 61 64 6d 33"),
        "example={example:?}"
    );
}
