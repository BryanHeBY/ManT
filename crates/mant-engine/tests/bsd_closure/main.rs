//! Regressions promoted from the final NetBSD and `DragonFly` BSD release audit.

#[path = "../support/fixtures.rs"]
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
    // The exact four-TP shape was checked with pinned CVS tree, HTML and
    // UTF-8. man_html.c::list_continues groups TP heads as one Bl-tag DL;
    // an authored `\ \ \ \(bu` head stays visible without becoming an entry.
    let items = section(document, "DESCRIPTION")
        .blocks
        .iter()
        .find_map(|block| match block {
            mant_ir::Block::DefinitionList { items, .. } if items.len() == 4 => Some(items),
            _ => None,
        })
        .expect("four tagged bullet rows");
    let bodies = [
        "Start your program",
        "Make your program stop",
        "Examine what has happened",
        "Change things in your program",
    ];
    for (item, body) in items.iter().zip(bodies) {
        assert_eq!(inline_text(&item.terms[0]).trim(), "•");
        assert!(item.entry.is_none());
        assert!(
            block_slice_text(&item.description).contains(body),
            "lost {body}"
        );
    }
    let query = common::query_for_document("gdb", document);
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query))
        .expect("serialize real gdb query");
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).expect("decode gdb");
    let decoded: mant_ir::ResolvedContent = decoded.into();
    for output in [
        mant_codec::encode::render_markdown(&decoded),
        mant_render::render_query_text(&decoded),
    ] {
        let mut previous = 0;
        for body in bodies {
            let next = output[previous..].find(body).map_or_else(
                || panic!("lost {body}: {output}"),
                |offset| previous + offset,
            );
            previous = next + body.len();
        }
    }
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
