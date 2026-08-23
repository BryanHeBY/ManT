//! Tests for rclone's official Go Windows release manual.
//!
//! The Pandoc-generated page exercises GNU verbatim font extensions that the
//! native parser compatibility layer must preserve without diagnostics.

use mant_engine::{render_excerpt_markdown, select_excerpt};
use mant_ir::Inline;

use crate::common::{self, block_slice_text, collect_sections, source_path_ends_with};
use crate::fixtures::{windows_release_manual, windows_release_query};

#[test]
fn keeps_the_large_release_topology_and_windows_specific_sections() {
    let document = windows_release_manual("rclone");
    assert_eq!(document.meta.title.as_deref(), Some("rclone"));
    assert_eq!(document.meta.manual_section.as_deref(), Some("1"));
    assert_eq!(document.meta.date.as_deref(), Some("July 31, 2026"));
    assert_eq!(document.meta.os.as_deref(), Some("User Manual"));
    assert_eq!(document.sections.len(), 191);
    assert!(source_path_ends_with(
        document,
        "windows-releases/rclone.1.zst"
    ));

    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    assert_eq!(sections.len(), 3_430);
    for title in [
        "rclone completion powershell",
        "Global Flags",
        "Microsoft OneDrive",
        "Local Filesystem",
        "Paths on Windows",
        "License",
    ] {
        assert!(
            sections.iter().any(|section| section.title == title),
            "missing reviewed rclone section {title}",
        );
    }
}

#[test]
fn preserves_windows_paths_and_powershell_commands() {
    let document = windows_release_manual("rclone");
    let paths = block_slice_text(&common::section(document, "Paths on Windows").blocks);
    assert!(paths.contains(r"C:\path\to\wherever"));
    assert!(paths.contains(r"\\server\share"));
    assert!(paths.contains(r"\\?\D:\some\very\long\path"));
    assert!(paths.contains(r"\\?\Volume{b75e2c83-0000-0000-0000-602f00000000}\some\path"));

    let powershell_section = common::section(document, "rclone completion powershell");
    let powershell = powershell_section
        .children
        .iter()
        .find(|section| section.title == "Synopsis")
        .map(|section| block_slice_text(&section.blocks))
        .expect("rclone PowerShell synopsis");
    assert!(powershell.contains("rclone completion powershell | Out-String | Invoke-Expression"));
}

#[test]
fn preserves_pandoc_verbatim_font_semantics() {
    let document = windows_release_manual("rclone");
    let mut code_segments = 0;
    let mut bold_code = false;
    let mut italic_code = false;
    for block in common::document_blocks(document) {
        common::visit_block_inlines(block, &mut |inline| match inline {
            Inline::Code { .. } => code_segments += 1,
            Inline::Strong { children } => {
                bold_code |= children
                    .iter()
                    .any(|child| matches!(child, Inline::Code { .. }));
            }
            Inline::Emphasis { children } => {
                italic_code |= children
                    .iter()
                    .any(|child| matches!(child, Inline::Code { .. }));
            }
            _ => {}
        });
    }

    assert!(code_segments > 1_000, "expected Pandoc verbatim code runs");
    assert!(bold_code, "expected a Pandoc bold-verbatim run");
    assert!(italic_code, "expected a Pandoc italic-verbatim run");
    assert!(document.diagnostics.iter().all(|diagnostic| {
        !diagnostic
            .message
            .starts_with("invalid escape sequence: \\f[")
    }));
}

#[test]
fn keeps_tier_table_text_blocks_in_their_own_columns() {
    let document = windows_release_manual("rclone");
    let tiers = block_slice_text(&common::section(document, "Tiers").blocks);
    for meaning in [
        "Production-grade, first-class",
        "Well-supported, minor gaps",
        "Works for many uses; known caveats",
        "Use with care; expect gaps/changes",
        "No longer maintained or supported",
    ] {
        assert!(tiers.contains(meaning), "missing tier meaning {meaning:?}");
    }
}

#[test]
fn renders_the_reviewed_windows_path_section_without_losing_backslashes() {
    let query = windows_release_query("rclone");
    let excerpt = select_excerpt(&query, &["paths-on-windows-3225".to_owned()])
        .expect("select rclone Paths on Windows");
    let markdown = render_excerpt_markdown(&excerpt);
    assert!(markdown.contains(r"`C:\path\to\wherever`"));
    assert!(markdown.contains(r"`\\server\share`"));
    assert!(markdown.contains(r"`\\?\D:\some\very\long\path`"));
}
