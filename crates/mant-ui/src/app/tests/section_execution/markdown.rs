//! Check both real Markdown readback chains against the original source owner.
use super::*;
use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};

fn find_path(sections: &[Section], target: &str) -> Option<Vec<usize>> {
    sections.iter().enumerate().find_map(|(index, section)| {
        if section.id.as_str() == target {
            Some(vec![index])
        } else {
            let children = find_path(&section.children, target)?;
            Some(std::iter::once(index).chain(children).collect())
        }
    })
}

fn section_at<'a>(mut sections: &'a [Section], path: &[usize]) -> &'a Section {
    let (&last, parents) = path.split_last().unwrap();
    for &index in parents {
        sections = &sections[index].children;
    }
    &sections[last]
}

fn assert_readback_owner(case: &Case, section: &Section) {
    // The Markdown reader may derive a different slug from the surviving
    // heading glyphs, e.g. source NEXT, executed EXT. The position under
    // the original parent and the title glyphs still identify the owner.
    assert_eq!(
        inline_plain_text(&section.heading.content).trim_start_matches(' '),
        case.heading(),
        "{}",
        case.id
    );

    // The exporter emits an independent heading with one CommonMark
    // separator, even when native BLOCK pre produced no vertical row.
    assert_eq!(
        section.spacing_before_lines, 1,
        "Markdown framing {}",
        case.id
    );

    let mut actual = Vec::new();
    for block in &section.blocks {
        let AstBlock::Paragraph {
            layout, children, ..
        } = block
        else {
            panic!(
                "unexpected Markdown block in filled source owner {}",
                case.id
            );
        };
        actual.extend(std::iter::repeat_n(
            String::new(),
            usize::from(layout.spacing_before_lines),
        ));
        actual.extend(inline_plain_text(children).split('\n').map(str::to_owned));
    }
    assert_eq!(actual, case.expected_body(), "Markdown BODY {}", case.id);

    // Framing between headings is Markdown's own policy. In contrast, none
    // of these BODYs begins with a blank: that is a source owner obligation
    // on both routes, including the real addressable parser's fallback.
    let AstBlock::Paragraph { children, .. } = section.blocks.last().unwrap() else {
        unreachable!();
    };
    assert_links(case, children);
}

#[test]
fn section_source_owners_survive_both_markdown_rounds() {
    for case in matrix() {
        let source = case.input();
        let path = find_path(&source.document.as_ref().unwrap().sections, "next").unwrap();
        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
            let mut current = source.clone();
            for cycle in 1..=2 {
                let artifact = render_addressable_markdown_with_options(&current, options);
                if cycle == 1 && options.preserve_anchors {
                    assert!(
                        artifact
                            .anchor_ranges()
                            .iter()
                            .any(|range| &artifact.text()[range.clone()] == r#"<a id="next"></a>"#),
                        "source navigation identity {}",
                        case.id
                    );
                }
                current = mant_loader::load_markdown_text(artifact.text(), None).unwrap();
                let document = current.document.as_ref().unwrap();
                assert_eq!(mant_ir::validate_document(document), []);
                assert_readback_owner(&case, section_at(&document.sections, &path));
                let route = if options.preserve_anchors {
                    "addressable"
                } else {
                    "default"
                };
                report(&case.id, &format!("markdown/{route}/{cycle}"), None);
            }
        }
    }
}
