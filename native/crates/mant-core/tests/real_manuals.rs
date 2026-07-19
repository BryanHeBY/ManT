//! Regression coverage migrated from the former TypeScript HTML parsers.
//!
//! These tests consume fixed compressed roff sources through libmandoc. They
//! protect source semantics and Mant's stable AST instead of renderer markup.

use std::{collections::HashSet, path::PathBuf, sync::OnceLock};

use mant_ast::{Block, Inline, MantDocument, QueryBundle, QuerySchema, Section, SourceFormat};
use mant_core::{build_outline, parse_manual_source, select_excerpt};

static LS: OnceLock<MantDocument> = OnceLock::new();
static GIT: OnceLock<MantDocument> = OnceLock::new();
static GCC: OnceLock<MantDocument> = OnceLock::new();
static CLANG: OnceLock<MantDocument> = OnceLock::new();
static TAR: OnceLock<MantDocument> = OnceLock::new();

const LS_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "AUTHOR",
    "REPORTING BUGS",
    "COPYRIGHT",
    "SEE ALSO",
];
const GIT_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "OPTIONS",
    "GIT COMMANDS",
    "HIGH-LEVEL COMMANDS (PORCELAIN)",
    "LOW-LEVEL COMMANDS (PLUMBING)",
    "GUIDES",
    "REPOSITORY, COMMAND AND FILE INTERFACES",
    "FILE FORMATS, PROTOCOLS AND OTHER DEVELOPER INTERFACES",
    "CONFIGURATION MECHANISM",
    "IDENTIFIER TERMINOLOGY",
    "SYMBOLIC IDENTIFIERS",
    "FILE/DIRECTORY STRUCTURE",
    "TERMINOLOGY",
    "ENVIRONMENT VARIABLES",
    "DISCUSSION",
    "SECURITY",
    "FURTHER DOCUMENTATION",
    "AUTHORS",
    "REPORTING BUGS",
    "SEE ALSO",
    "GIT",
    "NOTES",
];
const GCC_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "OPTIONS",
    "ENVIRONMENT",
    "BUGS",
    "FOOTNOTES",
    "SEE ALSO",
    "AUTHOR",
    "COPYRIGHT",
];
const CLANG_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "OPTIONS",
    "ENVIRONMENT",
    "BUGS",
    "SEE ALSO",
    "Author",
    "Copyright",
];
const TAR_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "NOTE",
    "DESCRIPTION",
    "OPTIONS",
    "RETURN VALUE",
    "SEE ALSO",
    "BUG REPORTS",
    "COPYRIGHT",
];
const TOPOLOGY_CASES: &[(&str, &[&str])] = &[
    ("ls", LS_SECTIONS),
    ("git", GIT_SECTIONS),
    ("gcc", GCC_SECTIONS),
    ("clang", CLANG_SECTIONS),
    ("tar", TAR_SECTIONS),
];

#[test]
fn fixed_roff_pages_keep_their_complete_section_topology() {
    for (name, expected_titles) in TOPOLOGY_CASES {
        let document = manual(name);
        assert_eq!(document.source.format, SourceFormat::Man, "fixture {name}");
        assert_eq!(
            document
                .sections
                .iter()
                .map(|section| section.title.as_str())
                .collect::<Vec<_>>(),
            *expected_titles,
            "fixture {name}",
        );

        let mut sections = Vec::new();
        collect_sections(&document.sections, &mut sections);
        assert!(
            sections
                .iter()
                .all(|section| !section.blocks.is_empty() || !section.children.is_empty()),
            "fixture {name} contains an empty section",
        );
        let ids: HashSet<&str> = sections.iter().map(|section| section.id.as_str()).collect();
        assert_eq!(ids.len(), sections.len(), "fixture {name} section IDs");
    }
}

#[test]
fn git_keeps_nested_sections_examples_and_inline_grouping() {
    let document = manual("git");
    assert_eq!(document.sections.len(), 24);

    let environment = section(document, "ENVIRONMENT VARIABLES");
    assert!(
        environment
            .children
            .iter()
            .any(|child| child.title == "Git Diffs")
    );
    assert!(!section(document, "GIT COMMANDS").blocks.is_empty());

    let preformatted = document_blocks(document)
        .into_iter()
        .filter_map(as_preformatted)
        .collect::<Vec<_>>();
    assert_eq!(preformatted.len(), 4);

    let synopsis = section(document, "SYNOPSIS");
    let synopsis_pre = synopsis
        .blocks
        .iter()
        .find_map(as_preformatted)
        .expect("Git synopsis display");
    assert!(contains_emphasis(synopsis_pre, "git"));
    assert!(count_line_breaks(synopsis_pre) > 3);
    assert!(!inline_text(synopsis_pre).contains("\n\n"));

    assert_preformatted(section(document, "OPTIONS"), "git --git-dir=a.git", 8);
    assert_preformatted(section(document, "CONFIGURATION MECHANISM"), "[core]", 4);
    let git_diffs = environment
        .children
        .iter()
        .find(|child| child.title == "Git Diffs")
        .expect("Git Diffs subsection");
    assert_preformatted(git_diffs, "path old-file", 8);

    let option_summary = section(document, "OPTIONS")
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. }
                if inline_text(children).contains("Prints the synopsis") =>
            {
                Some(children)
            }
            _ => None,
        })
        .expect("grouped -h description");
    assert!(contains_strong(option_summary, "--all"));
    assert!(contains_strong(option_summary, "-a"));
    assert!(inline_text(option_summary).contains("is given then all available"));
}

#[test]
fn gcc_keeps_large_hierarchy_fonts_and_pod_displays_without_control_text() {
    let document = manual("gcc");
    let options = section(document, "OPTIONS");
    assert_eq!(options.children.len(), 20);
    assert_eq!(options.children[0].title, "Option Summary");
    assert_eq!(
        options.children[1].title,
        "Options Controlling the Kind of Output"
    );
    assert!(
        options
            .children
            .iter()
            .any(|child| child.title == "Options to Request or Suppress Warnings")
    );

    let synopsis = section(document, "SYNOPSIS");
    let synopsis_inlines = synopsis
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. } => Some(children.as_slice()),
            _ => None,
        })
        .expect("GCC synopsis paragraph");
    assert!(contains_strong(synopsis_inlines, "-std="));
    assert!(contains_emphasis(synopsis_inlines, "standard"));

    let blocks = document_blocks(document);
    let displays = blocks
        .iter()
        .filter_map(|block| as_preformatted(block))
        .collect::<Vec<_>>();
    assert!(displays.len() > 250);
    let class_example = displays
        .iter()
        .find(|children| inline_text(children).contains("struct A { int a; };"))
        .expect("GCC class hierarchy example");
    assert!(inline_text(class_example).contains("struct C : B, A { };"));
    assert!(displays.iter().all(|children| {
        let text = inline_text(children);
        text.trim() != "CW" && text.trim() != "R"
    }));

    let phantom_paragraphs = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .filter(|text| matches!(text.trim(), "0" | "4"))
        .count();
    assert_eq!(
        phantom_paragraphs, 0,
        "roff request arguments leaked as text"
    );
}

#[test]
fn clang_keeps_option_pairs_and_discards_rst_control_dimensions() {
    let document = manual("clang");
    let stage_options = section(document, "Stage Selection Options");
    for (term, description) in [
        ("-E", "Run the preprocessor stage."),
        (
            "-fsyntax-only",
            "Run the preprocessor, parser and semantic analysis stages.",
        ),
        ("-c", "generating a target \".o\" object file"),
    ] {
        let item = definition_items(stage_options)
            .into_iter()
            .find(|item| item.terms.iter().any(|value| inline_text(value) == term))
            .unwrap_or_else(|| panic!("missing Clang option {term}"));
        assert!(block_slice_text(&item.description).contains(description));
    }

    let phantom_dimensions = document_blocks(document)
        .into_iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .filter(|text| {
            let text = text.trim();
            !text.is_empty()
                && text.ends_with('u')
                && text[..text.len() - 1]
                    .chars()
                    .all(|character| character.is_ascii_digit())
        })
        .count();
    assert_eq!(phantom_dimensions, 0, "roff .in arguments leaked as text");

    let language = section(document, "Language Selection and Mode Options");
    assert!(
        document_blocks_from_sections(std::slice::from_ref(language))
            .iter()
            .filter_map(|block| as_preformatted(block))
            .any(|children| inline_text(children).contains("iso9899:1990"))
    );
}

#[test]
fn ls_and_tar_keep_definition_lists_subsections_and_inline_styles() {
    let ls = manual("ls");
    let description = section(ls, "DESCRIPTION");
    let options = definition_items(description);
    assert_eq!(options.len(), 60);
    let all = options
        .iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| inline_text(term).contains("--all"))
        })
        .expect("ls --all option");
    assert!(
        all.terms
            .iter()
            .any(|term| contains_strong(term, "-a, --all"))
    );

    let exit = section(ls, "Exit status:");
    let exit_items = definition_items(exit);
    assert_eq!(exit_items.len(), 3);
    assert_eq!(inline_text(&exit_items[0].terms[0]), "0");
    assert!(block_slice_text(&exit_items[2].description).contains("serious trouble"));

    let tar = manual("tar");
    let synopsis = section(tar, "SYNOPSIS");
    assert_eq!(
        synopsis
            .children
            .iter()
            .map(|child| child.title.as_str())
            .collect::<Vec<_>>(),
        ["Traditional usage", "UNIX-style usage", "GNU-style usage"]
    );
    let tar_options = section(tar, "OPTIONS");
    assert!(tar_options.children.len() >= 15);
    assert!(
        definition_items(section(tar, "Operation mode"))
            .iter()
            .flat_map(|item| &item.terms)
            .any(|term| inline_text(term).contains("--create"))
    );
}

#[test]
fn fixed_roff_pages_do_not_leak_roff_or_html_markup_into_inline_text() {
    for name in ["ls", "git", "gcc", "clang", "tar"] {
        for block in document_blocks(manual(name)) {
            visit_block_inlines(block, &mut |inline| {
                let value = match inline {
                    Inline::Text { value } | Inline::Code { value } => value,
                    Inline::Strong { .. }
                    | Inline::Emphasis { .. }
                    | Inline::ExternalLink { .. }
                    | Inline::EmailLink { .. }
                    | Inline::ManualReference { .. }
                    | Inline::SectionReference { .. }
                    | Inline::Anchor { .. }
                    | Inline::LineBreak => return,
                };
                assert!(
                    !value.contains("\\f")
                        && !value.contains("\\(")
                        && !value.contains("\\*")
                        && !value.contains("<br")
                        && !value.contains("<b>")
                        && !value.contains("<i>"),
                    "fixture {name} leaked source markup: {value:?}",
                );
            });
        }
    }
}

#[test]
fn real_nested_manuals_support_outline_discovery_and_targeted_excerpts() {
    let query = manual_query("git");
    let outline = build_outline(&query).expect("Git outline");
    let git_diffs = &outline.nodes[15].children[3];
    assert_eq!(git_diffs.path, "16.4");
    assert_eq!(git_diffs.id, "git-diffs-28");
    assert_eq!(git_diffs.title, "Git Diffs");

    let excerpt = select_excerpt(&query, &["git-diffs-28".to_owned(), "16.4".to_owned()])
        .expect("Git Diffs excerpt");
    assert_eq!(excerpt.selections.len(), 1);
    assert_eq!(excerpt.selections[0].path, "16.4");
    assert_eq!(
        excerpt.selections[0].breadcrumbs[0].title,
        "ENVIRONMENT VARIABLES"
    );
    assert!(block_slice_text(&excerpt.selections[0].section.blocks).contains("GIT_EXTERNAL_DIFF"));
}

fn manual(name: &str) -> &'static MantDocument {
    let slot = match name {
        "ls" => &LS,
        "git" => &GIT,
        "gcc" => &GCC,
        "clang" => &CLANG,
        "tar" => &TAR,
        _ => panic!("unknown real-man fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&fixture_path(name))
            .unwrap_or_else(|error| panic!("parse {name} roff fixture: {error}"))
    })
}

fn manual_query(name: &str) -> QueryBundle {
    let manual = manual(name).clone();
    QueryBundle {
        schema: QuerySchema::V1,
        topic: name.to_owned(),
        section: manual.meta.section.clone(),
        manual: Some(manual),
        tldr: None,
    }
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("tests/fixtures/roff/real")
        .join(format!("{name}.1.gz"))
}

fn collect_sections<'a>(sections: &'a [Section], output: &mut Vec<&'a Section>) {
    for section in sections {
        output.push(section);
        collect_sections(&section.children, output);
    }
}

fn section<'a>(document: &'a MantDocument, title: &str) -> &'a Section {
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    sections
        .into_iter()
        .find(|section| section.title == title)
        .unwrap_or_else(|| panic!("missing section {title}"))
}

fn document_blocks(document: &MantDocument) -> Vec<&Block> {
    document_blocks_from_sections(&document.sections)
}

fn document_blocks_from_sections(sections: &[Section]) -> Vec<&Block> {
    let mut output = Vec::new();
    for section in sections {
        collect_blocks(&section.blocks, &mut output);
        output.extend(document_blocks_from_sections(&section.children));
    }
    output
}

fn collect_blocks<'a>(blocks: &'a [Block], output: &mut Vec<&'a Block>) {
    for block in blocks {
        output.push(block);
        match block {
            Block::List { items, .. } => {
                for item in items {
                    collect_blocks(&item.blocks, output);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    collect_blocks(&item.description, output);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_blocks(&cell.blocks, output);
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn definition_items(section: &Section) -> Vec<&mant_ast::DefinitionItem> {
    section
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn as_preformatted(block: &Block) -> Option<&[Inline]> {
    match block {
        Block::Preformatted { children, .. } => Some(children),
        _ => None,
    }
}

fn assert_preformatted(section: &Section, needle: &str, expected_indent: u16) {
    let block = section
        .blocks
        .iter()
        .find(|block| {
            as_preformatted(block).is_some_and(|children| inline_text(children).contains(needle))
        })
        .unwrap_or_else(|| panic!("missing preformatted text {needle:?} in {}", section.title));
    let Block::Preformatted {
        children, layout, ..
    } = block
    else {
        unreachable!()
    };
    assert!(inline_text(children).contains(needle));
    assert_eq!(layout.indent_columns, expected_indent);
}

fn contains_strong(children: &[Inline], expected: &str) -> bool {
    children.iter().any(|inline| match inline {
        Inline::Strong { children } => inline_text(children) == expected,
        Inline::Emphasis { children }
        | Inline::ExternalLink { children, .. }
        | Inline::EmailLink { children, .. }
        | Inline::ManualReference { children, .. }
        | Inline::SectionReference { children, .. } => contains_strong(children, expected),
        Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {
            false
        }
    })
}

fn contains_emphasis(children: &[Inline], expected: &str) -> bool {
    children.iter().any(|inline| match inline {
        Inline::Emphasis { children } => inline_text(children) == expected,
        Inline::Strong { children }
        | Inline::ExternalLink { children, .. }
        | Inline::EmailLink { children, .. }
        | Inline::ManualReference { children, .. }
        | Inline::SectionReference { children, .. } => contains_emphasis(children, expected),
        Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {
            false
        }
    })
}

fn count_line_breaks(children: &[Inline]) -> usize {
    children
        .iter()
        .map(|inline| match inline {
            Inline::LineBreak => 1,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => count_line_breaks(children),
            Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } => 0,
        })
        .sum()
}

fn inline_text(children: &[Inline]) -> String {
    children
        .iter()
        .map(|inline| match inline {
            Inline::Text { value } | Inline::Code { value } => value.clone(),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => inline_text(children),
            Inline::Anchor { .. } => String::new(),
            Inline::LineBreak => "\n".to_owned(),
        })
        .collect()
}

fn block_slice_text(blocks: &[Block]) -> String {
    blocks.iter().map(block_text).collect::<Vec<_>>().join("\n")
}

fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            inline_text(children)
        }
        Block::List { items, .. } => items
            .iter()
            .map(|item| block_slice_text(&item.blocks))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::DefinitionList { items, .. } => items
            .iter()
            .map(|item| {
                format!(
                    "{} {}",
                    item.terms
                        .iter()
                        .map(|term| inline_text(term))
                        .collect::<Vec<_>>()
                        .join(" "),
                    block_slice_text(&item.description),
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Table { rows, .. } => rows
            .iter()
            .flat_map(|row| &row.cells)
            .map(|cell| block_slice_text(&cell.blocks))
            .collect::<Vec<_>>()
            .join(" "),
        Block::Equation { value, .. } => value.clone(),
        Block::Unsupported { text, .. } => text.clone(),
        Block::VerticalSpace { .. } => String::new(),
    }
}

fn visit_block_inlines(block: &Block, visitor: &mut impl FnMut(&Inline)) {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            visit_inlines(children, visitor);
        }
        Block::List { items, .. } => {
            for item in items {
                for block in &item.blocks {
                    visit_block_inlines(block, visitor);
                }
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                for term in &item.terms {
                    visit_inlines(term, visitor);
                }
                for block in &item.description {
                    visit_block_inlines(block, visitor);
                }
            }
        }
        Block::Table { rows, .. } => {
            for cell in rows.iter().flat_map(|row| &row.cells) {
                for block in &cell.blocks {
                    visit_block_inlines(block, visitor);
                }
            }
        }
        Block::Equation { .. } | Block::VerticalSpace { .. } | Block::Unsupported { .. } => {}
    }
}

fn visit_inlines(children: &[Inline], visitor: &mut impl FnMut(&Inline)) {
    for inline in children {
        visitor(inline);
        match inline {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => visit_inlines(children, visitor),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
    }
}
