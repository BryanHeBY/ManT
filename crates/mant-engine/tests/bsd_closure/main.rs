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
fn dragonfly_gdb_preserves_consecutive_tp_option_aliases() {
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

    assert!(
        aliases.contains(&vec!["-symbols=file".into(), "-s file".into()]),
        "aliases={aliases:?}"
    );
    assert!(
        aliases.contains(&vec!["-exec=file".into(), "-e file".into()]),
        "aliases={aliases:?}"
    );
}
