//! Validate declared relationships independently from parser inference.
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    DefinitionCase, Diagnostic, DiagnosticLevel, Document, EntryContentSlice, EntryFacts,
    EntryInlineRoot, EntryOwner,
    visit::{self, Visit},
};

#[derive(Default)]
struct Owners<'a>(BTreeMap<&'a str, Vec<EntryOwner<'a>>>);
impl<'a> Visit<'a> for Owners<'a> {
    fn visit_definition_item(&mut self, item: &'a crate::DefinitionItem) {
        if let Some(facts) = &item.identity
            && has_relationship_facts(facts)
        {
            self.0
                .entry(facts.id.as_str())
                .or_default()
                .push(EntryOwner::Definition(item));
        }
        visit::walk_definition_item(self, item);
    }
    fn visit_list_item(&mut self, item: &'a crate::ListItem) {
        if let Some(facts) = &item.entry
            && has_relationship_facts(facts)
        {
            self.0
                .entry(facts.id.as_str())
                .or_default()
                .push(EntryOwner::List(item));
        }
        visit::walk_list_item(self, item);
    }
}

pub(crate) fn validate_relations(
    document: &Document,
    index: &crate::DocumentIndex,
) -> Vec<Diagnostic> {
    let mut owners = Owners::default();
    owners.visit_document(document);
    let mut diagnostics = Vec::new();
    let duplicate_ids: BTreeSet<_> = index
        .duplicates()
        .iter()
        .filter(|duplicate| duplicate.role == crate::IndexedRole::Entry)
        .map(|duplicate| duplicate.id.as_str())
        .collect();
    let mut eligible = BTreeSet::new();
    for (&id, records) in &owners.0 {
        for &owner in records {
            let facts = owner.facts().expect("collected fact owner");
            let bindings = valid_name_bindings(owner);
            if bindings.is_none() {
                diagnostics.push(finding(
                    "ir.invalid-entry-name-binding",
                    id,
                    "names must bind to matching text within authored forms",
                ));
            }
            let valid_groups = bindings
                .as_ref()
                .is_some_and(|bindings| groups_are_valid(facts, bindings));
            if !facts.alias_groups.is_empty() && !valid_groups {
                diagnostics.push(finding("ir.invalid-entry-alias-groups", id, "alias groups must be disjoint sets of at least two uniquely bound visible names"));
            }
            if records.len() == 1
                && !duplicate_ids.contains(id)
                && valid_groups
                && single_subject(facts)
                && bindings
                    .as_ref()
                    .is_some_and(|bindings| bindings.len() == facts.names.len())
            {
                eligible.insert(id);
            }
        }
    }
    let mut edges = BTreeMap::new();
    for (&id, records) in &owners.0 {
        for &owner in records {
            let facts = owner.facts().expect("collected fact owner");
            let Some(target) = &facts.alias_of else {
                continue;
            };
            let target = target.as_str();
            let compatible = owners
                .0
                .get(target)
                .and_then(|records| (records.len() == 1).then_some(records[0]))
                .and_then(EntryOwner::facts)
                .is_some_and(|other| other.role == facts.role && other.case == facts.case);
            if id == target || !eligible.contains(id) || !eligible.contains(target) || !compatible {
                diagnostics.push(finding("ir.invalid-entry-alias-of", id, "aliasOf requires a distinct, unique, same-role single-subject entry with compatible case policy"));
            } else {
                edges.insert(id, target);
            }
        }
    }
    // Each node has at most one outgoing relationship. Finish each chain once,
    // retaining its local positions to detect cycles without quadratic rescans.
    let mut finished = BTreeSet::new();
    for &start in edges.keys() {
        let mut positions = BTreeMap::new();
        let mut chain = Vec::new();
        let mut current = start;
        while !finished.contains(current) {
            if let Some(&cycle_start) = positions.get(current) {
                for &id in &chain[cycle_start..] {
                    diagnostics.push(finding(
                        "ir.cyclic-entry-alias",
                        id,
                        "aliasOf relationships must not form a cycle",
                    ));
                }
                break;
            }
            positions.insert(current, chain.len());
            chain.push(current);
            let Some(&next) = edges.get(current) else {
                break;
            };
            current = next;
        }
        finished.extend(chain);
    }
    diagnostics
}

fn finding(code: &str, id: &str, message: &str) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some(code.into()),
        message: format!("entry '{id}': {message}"),
        source: None,
    }
}

fn has_relationship_facts(facts: &EntryFacts) -> bool {
    !facts.name_bindings.is_empty() || !facts.alias_groups.is_empty() || facts.alias_of.is_some()
}

fn valid_name_bindings(owner: EntryOwner<'_>) -> Option<BTreeSet<usize>> {
    let facts = owner.facts()?;
    owner.forms()?;
    let mut names = BTreeSet::new();
    for binding in &facts.name_bindings {
        let expected = facts.names.get(binding.name)?;
        if !names.insert(binding.name) || binding.occurrences.is_empty() {
            return None;
        }
        for occurrence in &binding.occurrences {
            if occurrence
                .parts
                .iter()
                .any(|part| !inside_head(facts, part))
            {
                return None;
            }
            let nodes = owner.form(occurrence)?;
            if super::index::inline_text(&nodes) != *expected {
                return None;
            }
        }
    }
    Some(names)
}

fn inside_head(facts: &EntryFacts, piece: &EntryContentSlice) -> bool {
    if facts.forms.is_empty() {
        return matches!(piece.root, EntryInlineRoot::Term { .. });
    }
    facts.forms.iter().flat_map(|form| &form.parts).any(|head| {
        if piece.root != head.root || !piece.path.starts_with(&head.path) {
            return false;
        }
        match (&head.bytes, &piece.bytes) {
            (None, _) => true,
            (Some(a), Some(b)) => head.path == piece.path && a.start <= b.start && b.end <= a.end,
            (Some(_), None) => false,
        }
    })
}

fn groups_are_valid(facts: &EntryFacts, bound: &BTreeSet<usize>) -> bool {
    let mut grouped = BTreeSet::new();
    for group in &facts.alias_groups {
        if group.len() < 2 {
            return false;
        }
        for name in group {
            let mut matches =
                facts
                    .names
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| match facts.case {
                        DefinitionCase::Sensitive => *candidate == name,
                        DefinitionCase::Insensitive => candidate.eq_ignore_ascii_case(name),
                    });
            let Some((index, exact)) = matches.next() else {
                return false;
            };
            if exact != name
                || matches.next().is_some()
                || !bound.contains(&index)
                || !grouped.insert(index)
            {
                return false;
            }
        }
    }
    true
}

fn single_subject(facts: &EntryFacts) -> bool {
    facts.names.len() == 1
        || (facts.alias_groups.len() == 1 && facts.alias_groups[0].len() == facts.names.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Block, DefinitionItem, DefinitionRole, DocumentMeta, DocumentSource, EntryForm,
        EntryNameBinding, EntryNameEvidence, Inline, LayoutHint, SourceFormat,
    };

    fn entry(id: &str, names: &[&str]) -> DefinitionItem {
        DefinitionItem {
            identity: Some(EntryFacts {
                id: id.into(),
                role: DefinitionRole::Option,
                case: DefinitionCase::Sensitive,
                names: names.iter().map(|name| (*name).into()).collect(),
                value_domain: None,
                forms: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                name_bindings: names
                    .iter()
                    .enumerate()
                    .map(|(index, _)| EntryNameBinding {
                        name: index,
                        evidence: EntryNameEvidence::Declared,
                        occurrences: vec![EntryForm {
                            parts: vec![EntryContentSlice {
                                root: EntryInlineRoot::Term { index },
                                path: vec![0],
                                bytes: None,
                            }],
                        }],
                    })
                    .collect(),
            }),
            terms: names
                .iter()
                .map(|name| {
                    vec![Inline::Code {
                        value: (*name).into(),
                    }]
                })
                .collect(),
            description: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: "Body --hidden".into(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            inline_term: false,
            spacing_before_lines: None,
        }
    }

    fn document(items: Vec<DefinitionItem>) -> Document {
        Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            sections: Vec::new(),
            blocks: vec![Block::DefinitionList {
                items,
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
        }
    }

    fn codes(items: Vec<DefinitionItem>) -> Vec<String> {
        crate::validate_document(&document(items))
            .into_iter()
            .filter_map(|d| d.code)
            .collect()
    }

    #[test]
    fn explicit_groups_preserve_shared_body_without_merging_subjects() {
        let mut item = entry("time-bounds", &["-S", "--since", "-U", "--until"]);
        let body = item.description.clone();
        item.identity.as_mut().unwrap().alias_groups = vec![
            vec!["-S".into(), "--since".into()],
            vec!["-U".into(), "--until".into()],
        ];
        let doc = document(vec![item]);
        assert!(crate::validate_document(&doc).is_empty());
        let decoded: Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, decoded);
        let Block::DefinitionList { items, .. } = &decoded.blocks[0] else {
            panic!("definitions");
        };
        assert_eq!(items[0].description, body);
        assert_eq!(items[0].identity.as_ref().unwrap().alias_groups.len(), 2);
        let mut related = entry("related", &["--time"]);
        related.identity.as_mut().unwrap().alias_of = Some("time-bounds".into());
        assert!(
            codes(vec![items[0].clone(), related]).contains(&"ir.invalid-entry-alias-of".into())
        );
    }

    #[test]
    fn invalid_groups_never_establish_equivalence_or_hidden_names() {
        for groups in [
            vec![vec!["-S"]],
            vec![vec!["-S", "-S"]],
            vec![vec!["-S", "--hidden"]],
            vec![vec!["-S", "--since"], vec!["--since", "-S"]],
        ] {
            let mut item = entry("since", &["-S", "--since"]);
            item.identity.as_mut().unwrap().alias_groups = groups
                .into_iter()
                .map(|g| g.into_iter().map(str::to_owned).collect())
                .collect();
            assert!(codes(vec![item]).contains(&"ir.invalid-entry-alias-groups".into()));
        }
        let mut item = entry("case", &["-s", "-S"]);
        let facts = item.identity.as_mut().unwrap();
        facts.case = DefinitionCase::Insensitive;
        facts.alias_groups = vec![vec!["-s".into(), "-S".into()]];
        assert!(codes(vec![item]).contains(&"ir.invalid-entry-alias-groups".into()));
    }

    #[test]
    fn name_bindings_cannot_address_body_or_claim_different_text() {
        let mut item = entry("since", &["--since"]);
        item.identity.as_mut().unwrap().name_bindings[0].occurrences[0].parts[0].root =
            EntryInlineRoot::Block { index: 0 };
        assert!(codes(vec![item]).contains(&"ir.invalid-entry-name-binding".into()));
        let mut item = entry("since", &["--since"]);
        item.identity.as_mut().unwrap().names[0] = "--hidden".into();
        assert!(codes(vec![item]).contains(&"ir.invalid-entry-name-binding".into()));
    }

    #[test]
    fn forward_relationships_require_unique_compatible_targets_and_no_cycles() {
        let mut first = entry("first", &["--first"]);
        first.identity.as_mut().unwrap().alias_of = Some("second".into());
        let second = entry("second", &["--second"]);
        assert!(codes(vec![first.clone(), second.clone()]).is_empty());
        assert!(codes(vec![first.clone()]).contains(&"ir.invalid-entry-alias-of".into()));
        assert!(
            codes(vec![first.clone(), second.clone(), second.clone()])
                .contains(&"ir.invalid-entry-alias-of".into())
        );
        let mut incompatible = second.clone();
        incompatible.identity.as_mut().unwrap().role = DefinitionRole::Command;
        assert!(
            codes(vec![first.clone(), incompatible]).contains(&"ir.invalid-entry-alias-of".into())
        );
        let mut cyclic = second;
        cyclic.identity.as_mut().unwrap().alias_of = Some("first".into());
        assert_eq!(
            codes(vec![first, cyclic])
                .iter()
                .filter(|code| code.as_str() == "ir.cyclic-entry-alias")
                .count(),
            2
        );
        let mut item = entry("self", &["--self"]);
        item.identity.as_mut().unwrap().alias_of = Some("self".into());
        assert!(codes(vec![item]).contains(&"ir.invalid-entry-alias-of".into()));
    }
}
