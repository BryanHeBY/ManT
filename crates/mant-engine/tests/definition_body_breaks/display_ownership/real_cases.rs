//! Complete release fixtures for the display ownership boundary.
use std::io::Read;

use mant_loader::load_roff_bytes;

use super::check_display_owner;

#[test]
fn real_section_displays_do_not_become_part_of_the_previous_definition() {
    let fdio = load_roff_bytes(include_bytes!(
        "../../../../../tests/fixtures/roff/real/definition-displays/illumos-fdio.4i"
    ))
    .unwrap();
    for (name, display) in [
        ("FDIOSCHAR", "struct fd_char {"),
        ("FDSETDRIVECHAR", "struct fd_drive {"),
        ("FDIOCMD", "struct fd_cmd {"),
        ("FDRAW", "#define FDRAW_SPECIFY"),
    ] {
        check_display_owner(&fdio, name, display, false);
    }

    let mut source = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!(
            "../../../../../tests/fixtures/roff/real/definition-displays/freebsd-_umtx_op.2.gz"
        )[..],
    )
    .read_to_end(&mut source)
    .unwrap();
    let umtx = load_roff_bytes(&source).unwrap();
    for display in [
        "struct umutex {",
        "struct ucond {",
        "struct urwlock {",
        "struct _usem2 {",
        "struct _umtx_time {",
    ] {
        check_display_owner(&umtx, "USYNC_PROCESS_SHARED", display, false);
    }
}
