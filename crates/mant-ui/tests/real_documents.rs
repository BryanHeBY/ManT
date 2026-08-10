#![cfg(unix)]

//! Exercises terminal lowering against the same real roff corpus as mant-core.
//!
//! These tests intentionally avoid distribution-specific pixel snapshots.
//! They prove that large, structurally different manuals survive every common
//! terminal width while keeping all sidebar destinations addressable.

use std::path::{Path, PathBuf};

use mant_ast::{
    Block, Inline, MantDocument, QueryBundle, QueryInput, QueryRequest, QuerySchema, QueryView,
    RequestSchema, Section,
};
use mant_ui::DocumentView;

const REAL_MANUALS: &[&str] = &[
    "archlinux/clang.1.gz",
    "archlinux/gawk.1.zst",
    "archlinux/gcc.1.gz",
    "archlinux/git.1.gz",
    "archlinux/ls.1.gz",
    "archlinux/rsync.1.zst",
    "archlinux/sh.1p.gz",
    "archlinux/tar.1.gz",
    "debian/groff_man_style.7.gz",
    "debian/groff_me.7.gz",
    "debian/mt-gnu.1.gz",
    "debian/sh.1.gz",
    "fedora44/clang.1.zst",
    "fedora44/gcc.1.zst",
    "fedora44/git.1.zst",
    "fedora44/sh.1.zst",
    "fedora44/tar.1.zst",
];

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real")
        .join(relative)
}

fn project_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn view(relative: &str) -> DocumentView {
    let document = mant_core::parse_manual_source(&fixture(relative)).expect("parse real fixture");
    DocumentView::new(&QueryBundle {
        schema: QuerySchema::V4,
        label: relative.to_owned(),
        document: Some(document),
        tldr: None,
    })
}

fn visible_characters(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace() && !character.is_control())
        .collect()
}

#[derive(Debug)]
struct ExpectedFragment {
    value: String,
    independent: bool,
}

#[test]
fn real_manuals_render_at_narrow_and_wide_terminal_widths() {
    for &relative in REAL_MANUALS {
        let view = view(relative);
        assert!(
            !view.navigation().is_empty(),
            "{relative} has no navigation"
        );

        for width in [32, 80, 132] {
            let rendered = view.render(width);
            assert!(
                rendered.row_count >= view.section_count(),
                "{relative} lost content at width {width}"
            );
            for item in view.navigation() {
                assert!(
                    rendered.anchor_row(&item.target_id).is_some(),
                    "{relative} lost anchor {} for navigation {} at width {width}",
                    item.target_id,
                    item.id
                );
            }
            if let Some((row, character)) =
                rendered
                    .text
                    .lines
                    .iter()
                    .enumerate()
                    .find_map(|(row, line)| {
                        line.to_string()
                            .chars()
                            .find(|character| character.is_control())
                            .map(|character| (row, character))
                    })
            {
                panic!(
                    "{relative} emitted control U+{:04X} on row {row} at width {width}",
                    u32::from(character)
                );
            }
        }
    }
}

#[test]
fn semantic_definition_anchors_survive_real_tar_lowering() {
    let document = mant_core::parse_manual_source(&fixture("fedora44/tar.1.zst"))
        .expect("parse Fedora tar fixture");
    let acls_id = document
        .sections
        .iter()
        .flat_map(section_definitions)
        .find_map(|identity| {
            identity
                .names
                .iter()
                .any(|name| name == "--acls")
                .then(|| identity.id.clone())
        })
        .expect("tar --acls identity");
    let view = DocumentView::new(&QueryBundle {
        schema: QuerySchema::V4,
        label: "tar".to_owned(),
        document: Some(document),
        tldr: None,
    });

    for width in [32, 80, 132] {
        let rendered = view.render(width);
        assert!(rendered.anchor_row(&acls_id).is_some());
        assert!(
            !rendered.search("--acls").is_empty(),
            "tar --acls is visible but not searchable at width {width}"
        );
    }
}

#[test]
fn generated_git_presentation_requests_never_reach_terminal_text() {
    for relative in ["archlinux/git.1.gz", "fedora44/git.1.zst"] {
        let rendered = view(relative).render(132);
        let output = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(output.contains("The Git User's Manual[1]"), "{relative}");
        assert!(!output.contains("m[blue]"), "{relative}: {output}");
        assert!(!output.contains("s-2[1]s+2"), "{relative}: {output}");
    }
}

#[test]
fn real_manual_lowering_preserves_every_substantial_text_fragment() {
    for &relative in REAL_MANUALS {
        let document =
            mant_core::parse_manual_source(&fixture(relative)).expect("parse real fixture");
        let mut fragments = Vec::new();
        collect_document_fragments(&document, &mut fragments);
        let view = DocumentView::new(&QueryBundle {
            schema: QuerySchema::V4,
            label: relative.to_owned(),
            document: Some(document),
            tldr: None,
        });

        for width in [32, 80, 132] {
            let rendered = view.render(width);
            let output = visible_characters(
                &rendered
                    .text
                    .lines
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            let mut cursor = 0;
            for fragment in &fragments {
                let expected = visible_characters(&fragment.value);
                if expected.chars().count() < 4 {
                    continue;
                }
                if fragment.independent {
                    let query = fragment
                        .value
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    assert!(
                        !rendered.search(&query).is_empty(),
                        "{relative} lost independently wrapped table fragment {:?} at width {width}; word hits: {:?}",
                        fragment.value,
                        query
                            .split_whitespace()
                            .map(|word| (word, rendered.search(word).len()))
                            .collect::<Vec<_>>()
                    );
                    continue;
                }
                let Some(relative_position) = output[cursor..].find(&expected) else {
                    panic!(
                        "{relative} lost or reordered fragment {:?} at width {width}",
                        fragment.value
                    );
                };
                cursor += relative_position + expected.len();
            }
        }
    }
}

#[test]
fn self_hosted_markdown_manuals_use_the_same_terminal_pipeline() {
    for relative in ["docs/manuals/mant.md"] {
        let path = project_file(relative);
        let bundle = mant_core::query(&QueryRequest {
            schema: RequestSchema::V5,
            input: QueryInput::MarkdownFile {
                path: path.to_string_lossy().into_owned(),
            },
            view: QueryView::Full {},
        })
        .expect("query self-hosted Markdown manual");
        assert!(bundle.tldr.is_some(), "{relative} has no embedded tldr");
        let view = DocumentView::new(&bundle);

        for width in [32, 80, 132] {
            let rendered = view.render(width);
            assert_eq!(rendered.search("TLDR QUICK REFERENCE").len(), 1);
            assert!(
                !rendered.search("Synopsis").is_empty(),
                "{relative} lost its manual body at width {width}"
            );
            for item in view.navigation() {
                assert!(
                    rendered.anchor_row(&item.target_id).is_some(),
                    "{relative} lost target {} at width {width}",
                    item.target_id
                );
            }
        }
    }
}

fn collect_document_fragments(document: &MantDocument, output: &mut Vec<ExpectedFragment>) {
    collect_blocks(&document.blocks, output, false);
    for section in &document.sections {
        collect_section_fragments(section, output);
    }
}

fn collect_section_fragments(section: &Section, output: &mut Vec<ExpectedFragment>) {
    output.push(ExpectedFragment {
        value: section.title.clone(),
        independent: false,
    });
    collect_blocks(&section.blocks, output, false);
    for child in &section.children {
        collect_section_fragments(child, output);
    }
}

fn collect_blocks(blocks: &[Block], output: &mut Vec<ExpectedFragment>, independent: bool) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                collect_inlines(children, output, independent);
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_blocks(&item.blocks, output, independent);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &item.terms {
                        collect_inlines(term, output, independent);
                    }
                    collect_blocks(&item.description, output, independent);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_blocks(&cell.blocks, output, true);
                }
            }
            Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
                output.push(ExpectedFragment {
                    value: value.clone(),
                    independent,
                });
            }
            Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => {}
        }
    }
}

fn collect_inlines(inlines: &[Inline], output: &mut Vec<ExpectedFragment>, independent: bool) {
    for inline in inlines {
        match inline {
            Inline::Text { value } | Inline::Code { value } => output.push(ExpectedFragment {
                value: value.clone(),
                independent,
            }),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => {
                collect_inlines(children, output, independent);
            }
            Inline::Anchor { .. } | Inline::LineBreak => {}
        }
    }
}

fn section_definitions(section: &mant_ast::Section) -> Vec<&mant_ast::DefinitionIdentity> {
    let mut definitions = Vec::new();
    collect_block_definitions(&section.blocks, &mut definitions);
    for child in &section.children {
        definitions.extend(section_definitions(child));
    }
    definitions
}

fn collect_block_definitions<'a>(
    blocks: &'a [mant_ast::Block],
    definitions: &mut Vec<&'a mant_ast::DefinitionIdentity>,
) {
    for block in blocks {
        match block {
            mant_ast::Block::DefinitionList { items, .. } => {
                for item in items {
                    if let Some(identity) = &item.identity {
                        definitions.push(identity);
                    }
                    collect_block_definitions(&item.description, definitions);
                }
            }
            mant_ast::Block::List { items, .. } => {
                for item in items {
                    collect_block_definitions(&item.blocks, definitions);
                }
            }
            mant_ast::Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_block_definitions(&cell.blocks, definitions);
                }
            }
            mant_ast::Block::Paragraph { .. }
            | mant_ast::Block::Preformatted { .. }
            | mant_ast::Block::Equation { .. }
            | mant_ast::Block::VerticalSpace { .. }
            | mant_ast::Block::ThematicBreak { .. }
            | mant_ast::Block::Unsupported { .. } => {}
        }
    }
}
