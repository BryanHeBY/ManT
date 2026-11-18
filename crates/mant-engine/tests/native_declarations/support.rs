//! Shared fixture loading and assertions for native declaration matrices.

use std::ops::Range;

use mant_ir::{
    Block, DefinitionItem, Document, EntryOwner, ResolvedContent, inline_plain_text, visit,
};

use mant_protocol::{
    ContentSelector, DocumentResponse, EvidenceBasis, EvidenceClass, ExplanationContentRange,
    ExplanationOptions, ExplanationQuery, ExplanationSupport, QueryExcerpt, QueryExplanation,
};

#[path = "../support/json_roundtrip.rs"]
pub(super) mod json_roundtrip;

pub(super) fn load(source: &str) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let query = json_roundtrip::json_string_roundtrip(&original);
    assert_eq!(
        mant_ir::validate_document(query.document.as_ref().unwrap()),
        []
    );
    assert_eq!(query.document, original.document);
    query
}

pub(super) fn explain(query: &ResolvedContent, name: &str) -> mant_protocol::QueryExplanation {
    mant_query::explain_query(
        query,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap()
}

pub(super) const HEADER: &str =
    ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n";
pub(super) const LIST: &str = ".Bl -tag -width Ds\n";
pub(super) const COMMANDS: &str = ".It Cm n\n.It Cm N\nGROUP_BODY\n.El\n";

pub(super) fn source(before: &str, section: &str, head: &str) -> String {
    format!("{HEADER}{before}{section}{LIST}{head}{COMMANDS}")
}

pub(super) fn list_groups(document: &Document) -> Vec<Vec<(usize, usize)>> {
    struct Groups(Vec<Vec<(usize, usize)>>);
    impl<'ir> visit::Visit<'ir> for Groups {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::DefinitionList {
                declaration_groups, ..
            } = block
            {
                self.0.push(
                    declaration_groups
                        .iter()
                        .map(|group| (group.start_item, group.end_item))
                        .collect(),
                );
            }
            visit::walk_block(self, block);
        }
    }
    let mut groups = Groups(Vec::new());
    visit::Visit::visit_document(&mut groups, document);
    groups.0
}

pub(super) fn check_head(item: &DefinitionItem, form: &str, names: &[&str]) {
    let owner = EntryOwner::Definition(item);
    assert_eq!(owner.validated_names().unwrap(), names, "{form}");
    assert!(owner.facts().unwrap().alias_groups.is_empty(), "{form}");
    assert_eq!(
        owner
            .forms()
            .unwrap()
            .iter()
            .map(inline_plain_text)
            .collect::<Vec<_>>(),
        [form]
    );
}

pub(super) fn assert_search_fixture(
    source: &str,
    form: &str,
    names: &[&str],
    group_start: Option<usize>,
) -> ResolvedContent {
    let query = load(source);
    let document = query.document.as_ref().unwrap();
    let response = DocumentResponse::from(document);
    let decoded: DocumentResponse = roundtrip(&response);
    assert_eq!(Document::from(decoded), *document);
    let items = super::definitions(document);
    let [template, n, upper_n] = items.as_slice() else {
        panic!("{source}");
    };
    check_head(template, form, names);
    for (item, name) in [(n, "n"), (upper_n, "N")] {
        check_head(item, name, &[name]);
    }
    assert!(template.description.is_empty(), "{source}");
    assert!(n.description.is_empty(), "{source}");
    assert!(mant_ir::blocks_have_readable_content(&upper_n.description));
    assert_eq!(
        list_groups(document),
        [group_start
            .into_iter()
            .map(|start| (start, 3))
            .collect::<Vec<_>>()],
        "{source}"
    );

    for (expected, name) in [(n, "n"), (upper_n, "N")] {
        let result = explain(&query, name);
        let result: QueryExplanation = roundtrip(&result);
        assert_eq!(result.counts.direct_entry.total, 1, "{source}");
        let hit = result
            .evidence
            .iter()
            .find(|row| row.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(hit.source, expected.source);
        assert!(
            hit.bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        );
        assert!(
            hit.bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Form { .. }))
        );
        let shared = result.supports.iter().find_map(|support| {
            let ExplanationSupport::DeclarationGroup { group, .. } = support else {
                return None;
            };
            Some((group, support.items().unwrap()))
        });
        if let Some(start) = group_start {
            let (range, owners) = shared.unwrap();
            assert_eq!((range.start_item, range.end_item), (start, 3));
            assert_eq!(owners.last().unwrap(), *upper_n);
        } else {
            assert!(shared.is_none(), "{source}");
        }
        let excerpt =
            mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        let excerpt: QueryExcerpt = roundtrip(&excerpt);
        assert_eq!(
            mant_render::render_excerpt_text(&excerpt)
                .matches("GROUP_BODY")
                .count(),
            usize::from(name == "N"),
            "{source}"
        );
        assert_eq!(
            mant_render::render_explanation_text(&result)
                .matches("GROUP_BODY")
                .count(),
            usize::from(name == "N" || group_start.is_some()),
            "{source}"
        );
    }
    if names.is_empty() && !form.chars().any(char::is_control) {
        let template_only = explain(&query, form);
        assert!(
            template_only.evidence.iter().all(|hit| {
                hit.bases
                    .iter()
                    .all(|basis| !matches!(basis, EvidenceBasis::Name { .. }))
            }),
            "{source}"
        );
        assert!(template_only.supports.is_empty(), "{source}");
    }
    query
}

pub(super) fn check_occurrences(
    hit: &mant_protocol::ExplanationEvidence,
    name: &str,
    ranges: &[Range<usize>],
) {
    let matches = hit
        .bases
        .iter()
        .find_map(|basis| {
            if let EvidenceBasis::Name { matches } = basis {
                Some(matches)
            } else {
                None
            }
        })
        .expect("accepted name evidence");
    let [record] = matches.as_slice() else {
        panic!("expected one matched spelling for {name}");
    };
    assert_eq!(record.name, name);
    assert_eq!(record.occurrences.len(), ranges.len(), "{name}");
    let entry = hit.entry.as_ref().unwrap();
    let display = entry
        .name_bindings
        .iter()
        .find(|binding| entry.names[binding.name_index as usize] == name)
        .unwrap();
    assert_eq!(record.occurrences, display.occurrences, "{name}");
    for (position, expected) in record.occurrences.iter().zip(ranges) {
        let mut cursor = expected.start;
        let mut spelling = String::new();
        for part in &position.forms {
            assert_eq!(part.form_index, 0, "{name}");
            let text = inline_plain_text(part.resolve(&entry.forms).unwrap());
            let start = part.start_char as usize;
            let end = part.end_char as usize;
            assert_eq!(start, cursor, "{name}");
            cursor = end;
            spelling.extend(text.chars().skip(start).take(end - start));
        }
        assert_eq!(cursor, expected.end, "{name}");
        assert_eq!(spelling, name, "{name}");
        let mut cursor = expected.start;
        for part in &position.content {
            assert!(
                matches!(
                    part,
                    ExplanationContentRange::DefinitionTerm {
                        item_index: 0,
                        term_index: 0,
                        ..
                    }
                ),
                "{name}"
            );
            let range = part.char_range();
            assert_eq!(range.start, cursor, "{name}");
            cursor = range.end;
        }
        assert_eq!(cursor, expected.end, "{name}");
    }
}

pub(super) fn roundtrip<
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
>(
    original: &T,
) -> T {
    let decoded = serde_json::from_str(&serde_json::to_string(original).unwrap()).unwrap();
    assert_eq!(&decoded, original);
    decoded
}
