//! Regression coverage migrated from the former TypeScript HTML parsers.
//!
//! These tests consume fixed compressed roff sources through libmandoc. They
//! protect source semantics and `ManT`'s stable AST instead of renderer markup.

use std::{collections::HashSet, path::PathBuf, sync::OnceLock};

use mant_ast::{
    Block, ExcerptSelection, Inline, MantDocument, OutlineDetail, OutlineNode, QueryBundle,
    QuerySchema, SearchCase, SearchQuery, SearchScope, SearchSyntax, Section, SourceFormat,
};
use mant_core::{
    build_outline, build_outline_with_detail, parse_manual_source, search_query, select_excerpt,
};

static LS: OnceLock<MantDocument> = OnceLock::new();
static GIT: OnceLock<MantDocument> = OnceLock::new();
static GCC: OnceLock<MantDocument> = OnceLock::new();
static CLANG: OnceLock<MantDocument> = OnceLock::new();
static TAR: OnceLock<MantDocument> = OnceLock::new();
static FEDORA44_CLANG: OnceLock<MantDocument> = OnceLock::new();
static FEDORA44_GCC: OnceLock<MantDocument> = OnceLock::new();
static FEDORA44_GIT: OnceLock<MantDocument> = OnceLock::new();
static FEDORA44_TAR: OnceLock<MantDocument> = OnceLock::new();

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
fn fedora44_zstd_pages_keep_complete_sections_and_semantic_option_outlines() {
    let cases = [
        ("clang", 9, 78, "-std", "22"),
        ("gcc", 10, 2_731, "-Wsuggest-final-types", "gcc-16"),
        ("git", 24, 25, "--help", "Git 2.53.0"),
        ("tar", 9, 156, "--acls", "TAR"),
    ];

    for (name, section_count, option_count, known_option, expected_os) in cases {
        let document = fedora44_manual(name);
        assert_eq!(document.source.format, SourceFormat::Man, "fixture {name}");
        assert_eq!(document.sections.len(), section_count, "fixture {name}");
        assert_eq!(
            document.meta.section.as_deref(),
            Some("1"),
            "fixture {name}"
        );
        assert_eq!(
            document.meta.os.as_deref(),
            Some(expected_os),
            "fixture {name} metadata must not leak roff escapes",
        );
        assert!(
            document
                .source
                .path
                .as_deref()
                .is_some_and(|path| path.ends_with(".1.zst")),
            "fixture {name} must exercise the zstd source path",
        );

        let query = query_for_document(name, document);
        let outline = build_outline_with_detail(&query, OutlineDetail::Options)
            .unwrap_or_else(|error| panic!("build {name} option outline: {error}"));
        assert_eq!(
            count_outline_entries(&outline.nodes),
            option_count,
            "fixture {name} semantic option count",
        );
        assert!(
            find_outline_entry(&outline.nodes, known_option).is_some(),
            "fixture {name} must expose {known_option}",
        );

        assert_no_duplicate_vertical_spacing(&document.sections, name);
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

    let help = nested_definition_items(section(document, "OPTIONS"))
        .into_iter()
        .find(|item| {
            item.identity
                .as_ref()
                .is_some_and(|identity| identity.names.iter().any(|name| name == "--help"))
        })
        .expect("semantic --help option");
    let option_summary = help
        .description
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

    let ancillary = section(document, "Ancillary Commands");
    let ancillary_text = block_slice_text(&ancillary.blocks);
    assert!(ancillary_text.contains("git-config(1)"));
    assert!(ancillary_text.contains("git-fast-export(1)"));

    let outline = build_outline_with_detail(&manual_query("git"), OutlineDetail::Options)
        .expect("git option outline");
    assert!(find_outline_entry(&outline.nodes, "--help").is_some());
    assert!(find_outline_entry(&outline.nodes, "-C").is_some());
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

    let cxx_options = section(document, "Options Controlling C++ Dialect");
    let suggest_final_methods = nested_definition_items(cxx_options)
        .into_iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| inline_text(term).contains("-Wsuggest-final-methods"))
        })
        .expect("GCC -Wsuggest-final-methods option");
    assert_eq!(
        suggest_final_methods.spacing_before_lines,
        Some(1),
        "default man(7) paragraph distance must separate adjacent GCC options",
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

    let target_options = section(document, "Target Selection Options");
    assert_eq!(
        target_options.spacing_before_lines, 1,
        "the SS macro must retain its leading paragraph distance",
    );
    let target_lists = target_options
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        target_lists.len(),
        1,
        "Sphinx INDENT wrappers must remain one semantic option list",
    );
    assert!(target_lists[0].len() > 5);
    assert_eq!(target_lists[0][0].spacing_before_lines, Some(0));
    let target_list_layout = target_options
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("Target Selection definition layout");
    assert_eq!(
        target_list_layout.spacing_before_lines, 1,
        "the first option must retain TP spacing after introductory prose",
    );
    assert!(
        target_lists[0]
            .iter()
            .skip(1)
            .all(|item| item.spacing_before_lines == Some(1)),
        "default man(7) paragraph distance must survive INDENT wrappers",
    );

    let code_generation = section(document, "Code Generation Options");
    let first_code_generation_layout = code_generation
        .blocks
        .first()
        .and_then(|block| match block {
            Block::DefinitionList { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("Code Generation starts with an option list");
    assert_eq!(
        first_code_generation_layout.spacing_before_lines, 0,
        "a transparent INDENT wrapper must not add space before a section's first block",
    );

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
        assert_document_has_no_source_markup(name, manual(name));
    }
    for name in ["git", "gcc", "clang", "tar"] {
        assert_document_has_no_source_markup(&format!("fedora44/{name}"), fedora44_manual(name));
    }
}

#[test]
fn real_nested_manuals_support_outline_discovery_and_targeted_excerpts() {
    let query = manual_query("git");
    let outline = build_outline(&query).expect("Git outline");
    let git_diffs = &outline.nodes[15].children()[3];
    assert_eq!(git_diffs.path(), "16.4");
    assert_eq!(git_diffs.id(), "git-diffs-28");
    assert_eq!(git_diffs.title(), "Git Diffs");

    let excerpt = select_excerpt(&query, &["git-diffs-28".to_owned(), "16.4".to_owned()])
        .expect("Git Diffs excerpt");
    assert_eq!(excerpt.selections.len(), 1);
    let ExcerptSelection::ManualSection {
        path,
        breadcrumbs,
        section,
        ..
    } = &excerpt.selections[0]
    else {
        panic!("expected Git Diffs manual section");
    };
    assert_eq!(path, "16.4");
    assert_eq!(breadcrumbs[0].title, "ENVIRONMENT VARIABLES");
    assert!(block_slice_text(&section.blocks).contains("GIT_EXTERNAL_DIFF"));
}

#[test]
fn fedora44_zstd_tar_options_are_addressable_in_v2_outlines_and_excerpts() {
    let document = fedora44_manual("tar");
    let query = query_for_document("tar", document);
    let acls = semantic_definition_items(document)
        .into_iter()
        .find(|item| {
            item.identity
                .as_ref()
                .is_some_and(|identity| identity.names.iter().any(|name| name == "--acls"))
        })
        .expect("tar --acls semantic option");
    let identity = acls.identity.as_ref().expect("option identity");
    assert!(!identity.id.is_empty());

    let outline =
        build_outline_with_detail(&query, OutlineDetail::Options).expect("tar option outline");
    let outlined = find_outline_entry(&outline.nodes, "--acls").expect("outlined --acls");
    assert_eq!(outlined.id(), identity.id);

    let excerpt = select_excerpt(&query, &["acls".to_owned()]).expect("--acls excerpt by alias");
    assert!(matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::ManualEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|value| value.id == identity.id)
    ));
}

#[test]
fn fedora44_zstd_tar_search_maps_long_options_to_markdown_lines_and_selectable_nodes() {
    let query = query_for_document("tar", fedora44_manual("tar"));
    let result = search_query(
        &query,
        &SearchQuery {
            pattern: "--acls".to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 1,
            limit: 100,
            offset: 0,
        },
    )
    .expect("search tar --acls");

    assert!(result.total >= 1, "the option term should be searchable");
    let option = result
        .matches
        .iter()
        .find(|found| matches!(&found.node, mant_ast::SearchNode::ManualEntry { names, .. } if names.iter().any(|name| name == "--acls")))
        .expect("--acls option match");
    assert!(option.node.path().contains("/o"));
    assert!(option.markdown.start_line > 1);
    assert!(option.markdown.start_column > 0);
    assert!(option.preview.contains("--acls"));

    let excerpt = select_excerpt(&query, &[option.node.path().to_owned()])
        .expect("search node can be passed directly to --node");
    assert!(matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::ManualEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| identity.names.iter().any(|name| name == "--acls"))
    ));
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
    query_for_document(name, manual(name))
}

fn query_for_document(name: &str, document: &MantDocument) -> QueryBundle {
    let manual = document.clone();
    QueryBundle {
        schema: QuerySchema::V2,
        topic: name.to_owned(),
        section: manual.meta.section.clone(),
        manual: Some(manual),
        tldr: None,
    }
}

fn fedora44_manual(name: &str) -> &'static MantDocument {
    let slot = match name {
        "clang" => &FEDORA44_CLANG,
        "gcc" => &FEDORA44_GCC,
        "git" => &FEDORA44_GIT,
        "tar" => &FEDORA44_TAR,
        _ => panic!("unknown Fedora Linux 44 fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&fedora44_fixture_path(name))
            .unwrap_or_else(|error| panic!("parse Fedora Linux 44 {name} fixture: {error}"))
    })
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("tests/fixtures/roff/real")
        .join(format!("{name}.1.gz"))
}

fn fedora44_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("tests/fixtures/roff/real/fedora44")
        .join(format!("{name}.1.zst"))
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

fn nested_definition_items(section: &Section) -> Vec<&mant_ast::DefinitionItem> {
    document_blocks_from_sections(std::slice::from_ref(section))
        .into_iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn semantic_definition_items(document: &MantDocument) -> Vec<&mant_ast::DefinitionItem> {
    document_blocks(document)
        .into_iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.iter()),
            _ => None,
        })
        .flatten()
        .filter(|item| item.identity.is_some())
        .collect()
}

fn find_outline_entry<'a>(nodes: &'a [OutlineNode], name: &str) -> Option<&'a OutlineNode> {
    for node in nodes {
        if matches!(node, OutlineNode::ManualEntry { names, .. } if names.iter().any(|value| value == name))
        {
            return Some(node);
        }
        if let Some(found) = find_outline_entry(node.children(), name) {
            return Some(found);
        }
    }
    None
}

fn count_outline_entries(nodes: &[OutlineNode]) -> usize {
    nodes
        .iter()
        .map(|node| {
            usize::from(matches!(node, OutlineNode::ManualEntry { .. }))
                + count_outline_entries(node.children())
        })
        .sum()
}

fn assert_no_duplicate_vertical_spacing(sections: &[Section], fixture: &str) {
    for section in sections {
        assert_block_spacing_is_normalized(&section.blocks, fixture, &section.title);
        assert_no_duplicate_vertical_spacing(&section.children, fixture);
    }
}

fn assert_block_spacing_is_normalized(blocks: &[Block], fixture: &str, section: &str) {
    for pair in blocks.windows(2) {
        if matches!(pair[0], Block::VerticalSpace { .. }) {
            assert_eq!(
                block_spacing_before(&pair[1]),
                0,
                "fixture {fixture} section {section} stores one roff gap twice",
            );
        }
    }
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    assert_block_spacing_is_normalized(&item.blocks, fixture, section);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    assert_block_spacing_is_normalized(&item.description, fixture, section);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    assert_block_spacing_is_normalized(&cell.blocks, fixture, section);
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

fn block_spacing_before(block: &Block) -> u16 {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => layout.spacing_before_lines,
        Block::VerticalSpace { .. } => 0,
    }
}

fn assert_document_has_no_source_markup(name: &str, document: &MantDocument) {
    for block in document_blocks(document) {
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
                    && !value.contains("<i>")
                    && !value.contains(['\u{1d}', '\u{1e}', '\u{1f}']),
                "fixture {name} leaked source markup: {value:?}",
            );
        });
    }
}

fn as_preformatted(block: &Block) -> Option<&[Inline]> {
    match block {
        Block::Preformatted { children, .. } => Some(children),
        _ => None,
    }
}

fn assert_preformatted(section: &Section, needle: &str, expected_indent: u16) {
    let (children, indent) = find_preformatted(&section.blocks, needle, 0)
        .unwrap_or_else(|| panic!("missing preformatted text {needle:?} in {}", section.title));
    assert!(inline_text(children).contains(needle));
    assert_eq!(indent, expected_indent);
}

fn find_preformatted<'a>(
    blocks: &'a [Block],
    needle: &str,
    base_indent: u16,
) -> Option<(&'a [Inline], u16)> {
    for block in blocks {
        match block {
            Block::Preformatted {
                children, layout, ..
            } if inline_text(children).contains(needle) => {
                return Some((children, base_indent + layout.indent_columns));
            }
            Block::List { items, layout, .. } => {
                for item in items {
                    if let Some(found) =
                        find_preformatted(&item.blocks, needle, base_indent + layout.indent_columns)
                    {
                        return Some(found);
                    }
                }
            }
            Block::DefinitionList { items, layout, .. } => {
                for item in items {
                    if let Some(found) = find_preformatted(
                        &item.description,
                        needle,
                        base_indent + layout.indent_columns + 4,
                    ) {
                        return Some(found);
                    }
                }
            }
            Block::Table { rows, layout, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    if let Some(found) =
                        find_preformatted(&cell.blocks, needle, base_indent + layout.indent_columns)
                    {
                        return Some(found);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::Unsupported { .. } => {}
        }
    }
    None
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
