//! Exact original post-table HEAD names retain their read/explain owner.
use mant_ir::{DefinitionItem, EntryOwner, SourceSpan, visit::Visit};
use mant_protocol::{
    ContentSelector, EvidenceBasis, EvidenceClass, ExplanationOptions, ExplanationQuery,
};
#[derive(Default)]
struct Owners<'a>(Vec<&'a DefinitionItem>);
impl<'a> Visit<'a> for Owners<'a> {
    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        self.0.push(item);
        mant_ir::visit::walk_definition_item(self, item);
    }
}
fn assert_owner(content: &mant_ir::ResolvedContent, name: &str, line: u32) {
    let mut owners = Owners::default();
    owners.visit_document(content.document.as_ref().unwrap());
    let selected = owners
        .0
        .iter()
        .filter(|item| {
            item.entry
                .as_ref()
                .is_some_and(|facts| facts.names.iter().any(|n| n == name))
        })
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1, "one real post-table HEAD name {name}");
    let item = *selected[0];
    let owner = EntryOwner::Definition(item);
    let facts = owner.facts().unwrap();
    assert_eq!(item.source.as_ref().map(|source| source.line), Some(line));
    let bound = facts
        .name_bindings
        .iter()
        .flat_map(|binding| &binding.occurrences)
        .map(|form| mant_ir::inline_plain_text(&owner.form(form).unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(bound, [name]);
    let explanation = mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let direct = explanation
        .evidence
        .iter()
        .filter(|evidence| {
            evidence.class == EvidenceClass::DirectEntry
                && evidence
                    .bases
                    .iter()
                    .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        })
        .collect::<Vec<_>>();
    assert_eq!(direct.len(), 1);
    let source: Option<SourceSpan> = item.source;
    assert_eq!(direct[0].source, source);
    let excerpt =
        mant_query::select_excerpt(content, &[ContentSelector::path(direct[0].outline.path())])
            .unwrap();
    assert_eq!(
        mant_render::render_excerpt_text(&excerpt)
            .matches("BETA")
            .count(),
        1
    );
    let _: mant_protocol::QueryExcerpt =
        serde_json::from_str(&serde_json::to_string(&excerpt).unwrap()).unwrap();
}
#[test]
fn complete_names_read_back_from_the_original_head_instead_of_following_prose() {
    let cases = crate::cases::cases()
        .into_iter()
        .filter(|case| case.id.starts_with("T1-") && case.id.ends_with("-head"))
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 6);
    for case in cases {
        let line = case
            .source
            .lines()
            .position(|line| line == ".It ALPHA" || line == ".TP")
            .unwrap()
            + 1;
        let content = crate::json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        assert_owner(&content, &case.expected_label, u32::try_from(line).unwrap());
    }
}
