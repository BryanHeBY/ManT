//! Regressions promoted from the final NetBSD and `DragonFly` BSD release audit.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

use common::{block_slice_text, section};
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
