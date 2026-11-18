use std::{collections::HashSet, fmt::Write as _};

use mant_codec::encode::{MarkdownOptions, render_markdown, render_markdown_with_options};
use mant_ir::{Block, DocumentIndex, Inline, LinkTarget, ResolvedContent, Section};
use mant_loader::load_markdown_text;
use mant_protocol::{ContentSelector, EntryProjection, OutlineNode};
use mant_query::{build_outline_projection, select_excerpt};

use super::json_roundtrip::json_string_roundtrip;

fn assert_readable_outline(content: &ResolvedContent) {
    fn check(node: &OutlineNode, content: &ResolvedContent, ids: &mut HashSet<String>) {
        let id = node.id();
        assert!(
            ids.insert(id.to_owned()),
            "duplicate ID for {}",
            node.path()
        );
        let by_id = format!("id:{id}").parse::<ContentSelector>().unwrap();
        let by_path = format!("path:{}", node.path())
            .parse::<ContentSelector>()
            .unwrap();
        assert_eq!(
            select_excerpt(content, &[by_id]).unwrap(),
            select_excerpt(content, &[by_path]).unwrap(),
            "{}",
            node.path(),
        );
        for child in node.children() {
            check(child, content, ids);
        }
    }
    let transported = json_string_roundtrip(content);
    let outline = build_outline_projection(&transported, EntryProjection::All, None).unwrap();
    let mut ids = HashSet::new();
    for node in &outline.nodes {
        check(node, &transported, &mut ids);
    }
}

fn titles() -> Vec<String> {
    vec![
        "q".repeat(511),
        "a".repeat(512),
        "a".repeat(513),
        format!("{}z", "a".repeat(512)),
        format!("{}q", "界".repeat(170)),
        format!("{}xy", "界".repeat(170)),
        "界".repeat(171),
    ]
}

fn check_title_matrix(sections: &[Section], native: bool) {
    let titles = titles();
    for (section, title) in sections.iter().zip(&titles) {
        assert_eq!(section.heading.plain_text(), *title);
        if native && title.starts_with('界') {
            // The fixed CVS tree spells Unicode in native heading operands
            // as u754C escapes. Preserve that established slug as a fragment.
            let fragment = title.replace('界', "u754c-");
            assert!(
                section
                    .fragment_aliases
                    .iter()
                    .any(|alias| alias.as_str() == fragment.trim_end_matches('-'))
            );
        } else if title.len() <= 512 {
            assert_eq!(section.id.as_str(), title);
        } else {
            assert!(
                section
                    .fragment_aliases
                    .iter()
                    .any(|alias| alias.as_str() == title),
                "full fragment of {} bytes",
                title.len(),
            );
        }
    }
    assert_ne!(sections[2].id, sections[3].id);
    assert!(sections[7].id.ends_with("-2"));
    assert!(sections[15].id.ends_with("-10"));
    assert_eq!(
        sections[16..]
            .iter()
            .map(|section| section.id.as_str())
            .collect::<Vec<_>>(),
        ["short-2", "short", "short-3"],
    );
}

#[test]
fn markdown_heading_byte_boundaries_keep_titles_fragments_and_owners() {
    let titles = titles();
    let mut source = String::from("# Probe\n\n");
    for (index, title) in titles.iter().enumerate() {
        write!(source, "## {title}\n\nBODY{index}.\n\n").unwrap();
    }
    for index in 1..10 {
        write!(source, "## {}\n\nDUPLICATE{index}.\n\n", titles[1]).unwrap();
    }
    source.push_str("## Short-2\n\nFirst.\n\n## Short\n\nSecond.\n\n## Short\n\nThird.\n");
    let content = load_markdown_text(&source, None).unwrap();
    check_title_matrix(&content.document.as_ref().unwrap().sections, false);
    assert_readable_outline(&content);
    let after = load_markdown_text(&render_markdown(&content), None).unwrap();
    check_title_matrix(&after.document.as_ref().unwrap().sections, false);
    assert_readable_outline(&after);
}

#[test]
fn markdown_local_and_external_fragments_keep_their_owner_after_canonical_collisions() {
    let full = "a".repeat(513);
    // Occupy the first bounded spelling. This must not change the full slug
    // previously published by the next heading.
    let occupied = format!("{}-02425c0f5b0d", "a".repeat(499));
    let raw_duplicate = format!("{full}-2");
    let source = format!(
        "# Probe\n\n[First](#{full}) [Duplicate](#{raw_duplicate}) [Authored](#Mixed.Target)\n\n\
         ## {occupied}\n\nOccupied.\n\n## {full} {{#Mixed.Target}}\n\nFirst.\n\n\
         ## {full}\n\nSecond.\n",
    );
    let content = load_markdown_text(&source, None).unwrap();
    let document = content.document.as_ref().unwrap();
    let [existing, first, duplicate] = document.sections.as_slice() else {
        panic!("three original owners");
    };
    assert_eq!(existing.id.as_str(), occupied);
    assert!(first.id.ends_with("-2"));
    assert!(duplicate.id.ends_with("-3"));
    let index = DocumentIndex::build(document);
    assert_eq!(index.fragment_target(&full), Some(&first.id));
    assert_eq!(index.fragment_target(&raw_duplicate), Some(&duplicate.id));
    assert_eq!(index.fragment_target("Mixed.Target"), Some(&first.id));
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("navigation paragraph");
    };
    let links = children.iter().filter_map(|inline| match inline {
        Inline::Link {
            target: LinkTarget::Section { id },
            ..
        } => Some(id),
        _ => None,
    });
    assert_eq!(
        links.collect::<Vec<_>>(),
        [&first.id, &duplicate.id, &first.id]
    );
    let addressable = render_markdown_with_options(&content, MarkdownOptions::ADDRESSABLE);
    assert!(addressable.contains(&format!("<a id=\"{full}\"></a>")));
    assert!(addressable.contains(&format!("<a id=\"{raw_duplicate}\"></a>")));
    assert!(addressable.contains(&format!("[Duplicate](#{})", duplicate.id)));
    assert_readable_outline(&content);
}

#[test]
fn a_long_heading_attribute_keeps_its_spelling_as_a_single_fragment() {
    let raw = "x".repeat(513);
    let content = load_markdown_text(
        &format!("# Probe\n\n[Target](#{raw})\n\n## Caption {{#{raw}}}\n\nTARGETBODY.\n"),
        None,
    )
    .unwrap();
    let document = content.document.as_ref().unwrap();
    let section = &document.sections[0];
    assert_eq!(section.heading.plain_text(), "Caption");
    assert_eq!(
        section.fragment_aliases,
        [mant_ir::FragmentAlias::from(raw.as_str()), "caption".into()]
    );
    assert_eq!(
        DocumentIndex::build(document).fragment_target(&raw),
        Some(&section.id)
    );
    assert_readable_outline(&content);
}

#[cfg(feature = "roff")]
#[test]
fn roff_sh_and_ss_titles_obey_the_same_selection_budget() {
    // CVS man_term.c::pre_SH/pre_SS keeps heading operands separate from BODY.
    // The fixed tree and terminal showed the full text for each of these rows.
    let titles = titles();
    let mut source = String::from(".TH CHECK 1 \"2026-11-18\"\n.SH DESCRIPTION\n");
    for (index, title) in titles.iter().enumerate() {
        writeln!(source, ".SS \"{title}\"\nBODY{index}.").unwrap();
    }
    for index in 1..10 {
        writeln!(source, ".SS \"{}\"\nDUPLICATE{index}.", titles[1]).unwrap();
    }
    source.push_str(".SS Short-2\nFirst.\n.SS Short\nSecond.\n.SS Short\nThird.\n");
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    check_title_matrix(
        &content.document.as_ref().unwrap().sections[0].children,
        true,
    );
    assert_readable_outline(&content);
}

#[cfg(feature = "roff")]
#[test]
fn mdoc_sx_uses_the_final_bounded_id_and_keeps_duplicate_titles_ambiguous() {
    // CVS mdoc_html.c::mdoc_sx_pre emits a fragment for the authored title.
    let heading = "h".repeat(513);
    let duplicate = "j".repeat(513);
    let source = format!(
        ".Dd November 18, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd test\n\
         .Sh DESCRIPTION\n.Sx {heading}\n.Ss {heading}\nLONGOWNER.\n\
         .Ss {duplicate}\nFIRST.\n.Ss {duplicate}\nSECOND.\n.Sx {duplicate}\n",
    );
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let document = content.document.as_ref().unwrap();
    let description = &document.sections[1];
    let Block::Paragraph { children, .. } = &description.blocks[0] else {
        panic!("section reference");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: LinkTarget::Section { id }, .. } if id == &description.children[0].id
    )));
    assert_eq!(description.children[0].heading.plain_text(), heading);
    assert_eq!(
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code.as_deref() == Some("unresolved-section-reference"))
            .count(),
        1,
    );
    assert_readable_outline(&content);
}

#[cfg(feature = "roff")]
#[test]
fn generated_native_anchors_cannot_take_the_sections_long_fragment() {
    // CVS man_validate.c::post_SH and post_TP attach tags to separate HEADs.
    let full = "a".repeat(513);
    let source = format!(
        ".TH CHECK 1 \"2026-11-18\"\n.SH NAME\ncheck \\- test\n.SH {}\nSECTION\n\
         .SH OPTIONS\n.TP\n.B --{full}\nBODY\n",
        full.to_uppercase(),
    );
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let document = content.document.as_ref().unwrap();
    let index = DocumentIndex::build(document);
    assert_eq!(index.fragment_target(&full), Some(&document.sections[1].id));
    assert_eq!(
        index
            .fragment_target(&format!("{full}-2"))
            .map(mant_ir::NodeId::as_str),
        Some(format!("{full}-2").as_str()),
    );
    assert_readable_outline(&content);
}

#[cfg(feature = "roff")]
#[test]
fn native_heading_ids_reserve_later_literal_spellings_before_shortening() {
    // CVS man_validate.c::post_SH and mdoc_html.c::mdoc_sh_pre/mdoc_ss_pre
    // retain the literal heading target in both orders; Sx resolves separately.
    let long = "a".repeat(513);
    let literal = format!("{}-02425c0f5b0d", "a".repeat(499));
    for mdoc in [false, true] {
        for nested in [false, true] {
            native_heading_collision(&long, &literal, mdoc, nested, false);
        }
    }

    // An Em wrapper keeps the Unicode authored phrase; a bare native text
    // operand has CVS's escaped spelling and does not exercise this cut.
    let long = "界".repeat(171);
    let literal = format!("{}-9aca22c6f9ce", "界".repeat(166));
    for nested in [false, true] {
        native_heading_collision(&long, &literal, true, nested, true);
    }
}

#[cfg(feature = "roff")]
fn native_heading_collision(long: &str, literal: &str, mdoc: bool, nested: bool, styled: bool) {
    let (prefix, macro_name) = if mdoc {
        (
            ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n",
            if nested { "Ss" } else { "Sh" },
        )
    } else {
        (
            ".TH PROBE 1 \"2026-11-18\"\n.SH DESCRIPTION\n",
            if nested { "SS" } else { "SH" },
        )
    };
    let references = mdoc && !styled;
    for reverse in [false, true] {
        let mut source = prefix.to_owned();
        if references {
            writeln!(source, ".Sx {long}\n.Sx {literal}").unwrap();
        }
        let mut headings = [(long, "BODY-A"), (literal, "BODY-B")];
        if reverse {
            headings.reverse();
        }
        let style = if styled { "Em " } else { "" };
        for (title, body) in headings {
            writeln!(source, ".{macro_name} {style}{title}\n{body}").unwrap();
        }

        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = json_string_roundtrip(&query);
        let document = content.document.as_ref().unwrap();
        let description_index = usize::from(mdoc);
        let description = &document.sections[description_index];
        let siblings = if nested {
            &description.children[..]
        } else {
            &document.sections[description_index + 1..]
        };
        let long_section = &siblings[usize::from(reverse)];
        let literal_section = &siblings[usize::from(!reverse)];
        assert_eq!(long_section.heading.plain_text(), long);
        assert_eq!(literal_section.heading.plain_text(), literal);
        assert_eq!(literal_section.id.as_str(), literal);
        assert_ne!(long_section.id, literal_section.id);
        let index = DocumentIndex::build(document);
        assert_eq!(index.fragment_target(long), Some(&long_section.id));
        assert_eq!(index.fragment_target(literal), Some(&literal_section.id));
        if references {
            let Block::Paragraph { children, .. } = &description.blocks[0] else {
                panic!("Sx references belong to DESCRIPTION");
            };
            let targets = children
                .iter()
                .filter_map(|child| match child {
                    Inline::Link {
                        target: LinkTarget::Section { id },
                        ..
                    } => Some(id),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(targets, [&long_section.id, &literal_section.id]);
        }
        assert_literal_heading_owner(&content, literal);
    }
}

#[cfg(feature = "roff")]
fn assert_literal_heading_owner(content: &ResolvedContent, literal: &str) {
    let excerpt = select_excerpt(content, &[ContentSelector::id(literal)]).unwrap();
    let text = mant_render::render_excerpt_text(&excerpt);
    assert!(text.contains("BODY-B"));
    assert!(!text.contains("BODY-A"));
    assert_readable_outline(content);
    let exported = render_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    let anchor = format!("<a id=\"{literal}\"></a>");
    assert_eq!(exported.matches(&anchor).count(), 1);
    let (_, destination) = exported.split_once(&anchor).unwrap();
    assert_eq!(
        destination.lines().find(|line| line.starts_with("BODY-")),
        Some("BODY-B")
    );
}

#[cfg(feature = "roff")]
#[test]
fn libstdcxx_manual_keeps_the_complete_long_subsections_selectable() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/section-navigation/archlinux-gp_hash_table.3.gz");
    let document = mant_loader::parse_manual_source(&path).unwrap();
    let section = &document.sections[3].children[11];
    assert_eq!(section.heading.plain_text().chars().count(), 763);
    assert!(section.heading.plain_text().contains("::gp_hash_table"));
    assert!(
        section
            .fragment_aliases
            .iter()
            .any(|alias| alias.as_str().len() == 697)
    );
    let index = DocumentIndex::build(&document);
    for heading in &document.sections[3].children[..12] {
        let [fragment] = heading.fragment_aliases.as_slice() else {
            panic!("each previously oversized ID survives as one fragment");
        };
        assert!(fragment.as_str().len() > 512);
        assert_eq!(index.fragment_target(fragment.as_str()), Some(&heading.id));
    }
    assert_readable_outline(&ResolvedContent {
        label: "gp_hash_table".into(),
        address: None,
        document: Some(document),
        tldr: None,
    });
}
