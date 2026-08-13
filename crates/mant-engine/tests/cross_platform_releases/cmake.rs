//! Tests for `CMake`'s byte-identical Windows and Linux toolchain reference.

use crate::common::{self, block_slice_text};
use crate::fixtures::cross_platform_release_manual;

const CMAKE_TOOLCHAIN_SECTIONS: &[&str] = &[
    "NAME",
    "INTRODUCTION",
    "LANGUAGES",
    "VARIABLES AND PROPERTIES",
    "TOOLCHAIN FEATURES",
    "CROSS COMPILING",
    "Copyright",
];

#[test]
fn keeps_cross_compiler_and_windows_toolchain_sections() {
    let document = cross_platform_release_manual("cmake-toolchains");
    common::assert_section_topology(
        "cross-platform-releases/cmake-toolchains",
        document,
        CMAKE_TOOLCHAIN_SECTIONS,
    );
    assert_eq!(document.meta.section.as_deref(), Some("7"));
    assert_eq!(document.meta.date.as_deref(), Some("July 31, 2026"));
    assert_eq!(document.meta.os.as_deref(), Some("4.4.2"));

    let cross_compiling = common::section(document, "CROSS COMPILING");
    for title in [
        "Cross Compiling using Clang",
        "Cross Compiling for Windows CE",
        "Cross Compiling for Android with the NDK",
        "Cross Compiling for Emscripten",
    ] {
        assert!(
            cross_compiling
                .children
                .iter()
                .any(|child| child.title == title),
            "missing reviewed CMake subsection {title}",
        );
    }
    assert!(block_slice_text(&cross_compiling.blocks).contains("--toolchain path/to/file"));
}

#[test]
fn does_not_leak_roff_markup_or_duplicate_spacing() {
    let document = cross_platform_release_manual("cmake-toolchains");
    common::assert_document_has_no_source_markup(
        "cross-platform-releases/cmake-toolchains",
        document,
    );
    common::assert_no_duplicate_vertical_spacing(
        &document.sections,
        "cross-platform-releases/cmake-toolchains",
    );
}
