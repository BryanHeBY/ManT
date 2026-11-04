//! Public queries keep original synopsis sections and exact inline occurrences.

use std::{collections::BTreeSet, ops::Range};

use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};
use mant_ir::{Block, Inline, LinkTarget, ReferenceTargetType, ResolvedContent, Section};
use mant_protocol::{
    ContentSelector, EntryProjection, EvidenceClass, ExcerptSelection, ExplanationContent,
    ExplanationOptions, ExplanationQuery, ReferenceCount, ReferenceProjection,
    ReferenceProjectionMode, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};

use super::Case;

fn section_path(case: &Case) -> String {
    let mut index = 0;
    for line in case.source.lines() {
        if line.starts_with(".Sh ") {
            index += 1;
        }
        if line == ".Sh SYNOPSIS" {
            return index.to_string();
        }
    }
    panic!("{}: complete source must declare SYNOPSIS", case.id);
}

fn section<'a>(content: &'a ResolvedContent, path: &str) -> &'a Section {
    let index = path.parse::<usize>().unwrap() - 1;
    let section = &content.document.as_ref().unwrap().sections[index];
    assert_eq!(section.heading.plain_text(), "SYNOPSIS");
    section
}

fn assert_read(content: &ResolvedContent, selector: ContentSelector, expected: &Section) {
    let excerpt = mant_query::select_excerpt(content, &[selector]).unwrap();
    let [ExcerptSelection::DocumentSection { section, .. }] = excerpt.selections.as_slice() else {
        panic!("read must select the original section, without an invented entry");
    };
    assert_eq!(section, expected);
}

fn assert_references(content: &ResolvedContent, case: &Case, path: &str) {
    let outline = mant_query::build_outline_with_references(
        content,
        EntryProjection::All,
        Some(ContentSelector::path(path)),
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::Manual],
            ..ReferenceProjection::default()
        },
    )
    .unwrap();
    let targets = case
        .links
        .iter()
        .map(|link| (&link.name, &link.section))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        outline.references.occurrences,
        ReferenceCount::Exact {
            value: u64::try_from(case.links.len()).unwrap()
        }
    );
    assert_eq!(
        outline.references.targets,
        ReferenceCount::Exact {
            value: u64::try_from(targets.len()).unwrap()
        }
    );
    assert_eq!(outline.references.records.len(), case.links.len());
    for (record, expected) in outline.references.records.iter().zip(&case.links) {
        let target = LinkTarget::Manual {
            name: expected.name.clone(),
            manual_section: Some(expected.section.clone()),
        };
        assert_eq!(record.target, target, "{}", case.id);
        assert_eq!(record.label, expected.label, "{}", case.id);
        assert!(!record.label_truncated);
        assert_eq!(
            record.owner, None,
            "ordinary inline owners carry no EntryFacts"
        );
        assert_eq!(record.source_read, ContentSelector::path(path));
        let Some(Inline::Link {
            target: original,
            children,
            ..
        }) = record
            .origin
            .resolve_link(content.document.as_ref().unwrap())
        else {
            panic!("reference record must resolve its exact original occurrence");
        };
        assert_eq!(original, &target);
        assert_eq!(mant_ir::inline_plain_text(children), expected.label);
        assert_read(content, record.source_read.clone(), section(content, path));
    }
    for pair in outline.references.records.windows(2) {
        assert_ne!(
            pair[0].origin, pair[1].origin,
            "duplicates retain separate source locations"
        );
    }
}

fn assert_explanation(content: &ResolvedContent, case: &Case, path: &str, pattern: &str) {
    let response = mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: pattern.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let [evidence] = response.evidence.as_slice() else {
        panic!(
            "{}: one original synopsis paragraph must support {pattern:?}",
            case.id
        );
    };
    assert_eq!(evidence.class, EvidenceClass::ContextMention);
    assert_eq!(evidence.entry, None);
    assert_eq!(evidence.outline.path(), path);
    assert_eq!(evidence.outline.title(), "SYNOPSIS");
    let expected_path = format!("sections/s{}/b0", path.parse::<usize>().unwrap() - 1);
    assert_eq!(evidence.block_path.as_deref(), Some(expected_path.as_str()));
    let original =
        mant_query::resolve_explanation_block(content.document.as_ref().unwrap(), &expected_path)
            .unwrap();
    assert_eq!(Some(original), section(content, path).blocks.first());
    let Some(ExplanationContent::Block { block }) = &evidence.content else {
        panic!("ordinary synopsis evidence must return its complete original block");
    };
    assert_eq!(block, original);
    let expected = case.rows.join("\n");
    let first = expected.find(pattern).unwrap();
    let start = expected[..first].chars().count();
    let [preview] = evidence.previews.as_slice() else {
        panic!("one original block preview");
    };
    assert_eq!(preview.text, expected);
    assert_eq!(usize::try_from(preview.match_start_char).unwrap(), start);
    assert_eq!(
        usize::try_from(preview.match_end_char).unwrap(),
        start + pattern.chars().count()
    );
}

fn visible_position(line: &str, source: Range<usize>) -> (String, Range<usize>) {
    // Interpret the complete actual row, retaining its Markdown style
    // delimiters and source-map anchors. Expected columns are computed
    // separately from the frozen native rows, never from a reported hit.
    let complete = format!("{line}\n");
    let mut visible = String::new();
    let mut position = None;
    for (event, range) in pulldown_cmark::Parser::new(&complete).into_offset_iter() {
        let (pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text)) = event else {
            continue;
        };
        if range.start <= source.start && source.end <= range.end {
            assert_eq!(
                &complete[range.clone()],
                text.as_ref(),
                "selected source labels have no escape expansion"
            );
            let start =
                visible.chars().count() + complete[range.start..source.start].chars().count();
            position = Some(start..start + complete[source.clone()].chars().count());
        }
        visible.push_str(&text);
    }
    (
        visible,
        position.expect("complete label lies within original visible text"),
    )
}

fn assert_search(content: &ResolvedContent, case: &Case, path: &str, pattern: &str) {
    let expected = case
        .rows
        .iter()
        .flat_map(|row| {
            row.match_indices(pattern).map(move |(byte, found)| {
                let start = row[..byte].chars().count();
                (row.as_str(), start..start + found.chars().count())
            })
        })
        .collect::<Vec<_>>();
    let response = mant_query::search_query(
        content,
        &SearchQuery {
            pattern: pattern.into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 20,
            offset: 0,
        },
    )
    .unwrap();
    let actual = response
        .matches
        .iter()
        .flat_map(|hit| {
            assert_eq!(hit.outline.path(), path);
            assert!(!hit.occurrences_truncated);
            hit.occurrences.iter()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len(), "{}: {pattern}", case.id);
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    for (occurrence, (expected_row, expected_range)) in actual.into_iter().zip(expected) {
        assert_eq!(occurrence.matched_text, pattern);
        let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
        let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(&artifact.text()[start..end], pattern);
        let line_start = artifact.text()[..start]
            .rfind('\n')
            .map_or(0, |byte| byte + 1);
        let line_end = artifact.text()[end..]
            .find('\n')
            .map_or(artifact.text().len(), |byte| end + byte);
        let (row, range) = visible_position(
            &artifact.text()[line_start..line_end],
            start - line_start..end - line_start,
        );
        assert_eq!(row, expected_row, "{}: {pattern}", case.id);
        assert_eq!(range, expected_range, "{}: {pattern}", case.id);
        assert_eq!(
            usize::try_from(occurrence.markdown.start_column).unwrap(),
            artifact.text()[line_start..start].chars().count() + 1
        );
        assert_eq!(
            usize::try_from(occurrence.markdown.end_column).unwrap(),
            artifact.text()[line_start..end].chars().count() + 1
        );
    }
}

#[test]
fn synopsis_queries_retain_original_reference_occurrences_sections_and_complete_names() {
    // Reuse four complete sources whose five pinned profiles precede these
    // assertions. Native row/label expectations stay in cases.json. These
    // inline Nm/Xo/Vt scopes have real paragraphs, not semantic entry facts.
    for case in super::cases().into_iter().filter(|case| {
        matches!(
            case.id.as_str(),
            "original-nm-xo-lost-glyph"
                | "pair-linked-prefix-name"
                | "pair-duplicate-linked-names"
                | "pair-linked-type-tail"
        )
    }) {
        let content = super::roundtrip(&case);
        let path = section_path(&case);
        let selected = section(&content, &path);
        let [
            Block::Paragraph {
                children, source, ..
            },
        ] = selected.blocks.as_slice()
        else {
            panic!("{}: original paragraph owner", case.id);
        };
        let owner_line = case.source.lines().position(|line| line == ".Xo").unwrap() + 1;
        assert_eq!(source.unwrap().line, u32::try_from(owner_line).unwrap());
        assert_eq!(source.unwrap().column, 2);
        assert_eq!(mant_ir::inline_plain_text(children), case.rows.join("\n"));
        assert_read(&content, ContentSelector::path(path.as_str()), selected);
        assert_references(&content, &case, &path);
        let mut patterns = case
            .links
            .iter()
            .map(|link| link.label.as_str())
            .collect::<BTreeSet<_>>();
        for word in ["name", "PREFIX"] {
            if case.rows.iter().any(|row| row.contains(word)) {
                patterns.insert(word);
            }
        }
        for pattern in patterns {
            assert_explanation(&content, &case, &path, pattern);
            assert_search(&content, &case, &path, pattern);
        }
    }
}

#[test]
fn default_manual_markdown_keeps_complete_labels_and_word_boundaries_without_targets() {
    for case in super::cases().into_iter().filter(|case| {
        matches!(
            case.id.as_str(),
            "pair-linked-prefix-name" | "pair-duplicate-linked-names" | "pair-linked-type-tail"
        )
    }) {
        let content = super::roundtrip(&case);
        let markdown = mant_codec::encode::render_markdown(&content);
        assert!(
            !markdown.contains("man:"),
            "default body links are transparent"
        );
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_eq!(super::rows(&read), case.portable_rows);
        super::assert_reading_facts(&read, &case, &case.portable_font_words, &[]);
        let mut facts = super::ReadingFacts::default();
        mant_ir::visit::Visit::visit_section(&mut facts, section(&read, &section_path(&case)));
        let text = facts
            .scalars
            .iter()
            .map(|(character, _)| character)
            .collect::<String>();
        // Counts come from original source-owned occurrences. Default
        // Markdown must not invent syntax bytes or glue the adjacent name.
        for label in case
            .links
            .iter()
            .map(|link| &link.label)
            .collect::<BTreeSet<_>>()
        {
            assert_eq!(
                text.match_indices(label).count(),
                case.links
                    .iter()
                    .filter(|link| &link.label == label)
                    .count()
            );
        }
    }
}
