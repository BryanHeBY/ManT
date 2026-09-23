//! Validate declared relationships independently from parser inference.
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ContentContext, ContentReadError, Diagnostic, DiagnosticLevel, Document, EntryContentSlice,
    EntryFacts, EntryOwner, NameCase,
    visit::{self, Visit},
};

#[derive(Default)]
struct Owners<'a>(BTreeMap<&'a str, Vec<EntryOwner<'a>>>);

/// One rejected relation or visible-name binding, attributed to its owner.
/// Producers may remove the rejected field without parsing diagnostic prose;
/// consumers must not use a rejected relation as evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRelationIssue {
    /// Canonical ID of the content owner with the rejected fact.
    pub owner: crate::NodeId,
    /// The independent fact or relationship that failed validation.
    pub kind: EntryRelationIssueKind,
}

/// Source-neutral entry relationship failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryRelationIssueKind {
    /// A selectable name does not bind to its original visible head.
    NameBinding,
    /// A group is incomplete, overlapping, or not bound to visible names.
    AliasGroups,
    /// The target is absent, ambiguous, self-referential or incompatible.
    AliasOf,
    /// The relationship participates in a directed cycle.
    Cycle,
}

impl EntryRelationIssue {
    /// The same structured diagnostic emitted by document validation.
    #[must_use]
    pub fn diagnostic(&self) -> Diagnostic {
        let (code, message) = match self.kind {
            EntryRelationIssueKind::NameBinding => (
                "ir.invalid-entry-name-binding",
                "names must bind to matching text within authored forms",
            ),
            EntryRelationIssueKind::AliasGroups => (
                "ir.invalid-entry-alias-groups",
                "alias groups must be disjoint sets of at least two uniquely bound visible names",
            ),
            EntryRelationIssueKind::AliasOf => (
                "ir.invalid-entry-alias-of",
                "aliasOf requires a distinct, unique, same-role single-subject entry with compatible case policy",
            ),
            EntryRelationIssueKind::Cycle => (
                "ir.cyclic-entry-alias",
                "aliasOf relationships must not form a cycle",
            ),
        };
        Diagnostic {
            impact: crate::DiagnosticImpact::SemanticCoverage,
            level: DiagnosticLevel::Warning,
            code: Some(code.into()),
            message: format!("entry '{}': {message}", self.owner),
            source: None,
            coverage_scope: None,
        }
    }
}

/// Check explicit relationships without changing content or deriving aliases.
/// Forward references are resolved against this complete document snapshot.
#[must_use]
pub fn entry_relation_issues(document: &Document) -> Vec<EntryRelationIssue> {
    relation_issues(
        document,
        document.content(),
        &crate::DocumentIndex::build(document),
    )
}
impl<'a> Visit<'a> for Owners<'a> {
    fn visit_definition_item(&mut self, item: &'a crate::DefinitionItem) {
        if let Some(facts) = &item.entry
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

pub(crate) fn relation_issues(
    document: &Document,
    content: ContentContext<'_>,
    index: &crate::DocumentIndex,
) -> Vec<EntryRelationIssue> {
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
            let bindings = valid_name_bindings(content, owner)
                .expect("validated document content resolves in its own store");
            if bindings.is_none() {
                diagnostics.push(EntryRelationIssue {
                    owner: id.into(),
                    kind: EntryRelationIssueKind::NameBinding,
                });
            }
            let valid_groups = bindings
                .as_ref()
                .is_some_and(|bindings| groups_are_valid(facts, bindings));
            if !facts.alias_groups.is_empty() && !valid_groups {
                diagnostics.push(EntryRelationIssue {
                    owner: id.into(),
                    kind: EntryRelationIssueKind::AliasGroups,
                });
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
                .is_some_and(|other| other.kind == facts.kind && other.case == facts.case);
            if id == target || !eligible.contains(id) || !eligible.contains(target) || !compatible {
                diagnostics.push(EntryRelationIssue {
                    owner: id.into(),
                    kind: EntryRelationIssueKind::AliasOf,
                });
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
        let mut chain: Vec<&str> = Vec::new();
        let mut current = start;
        while !finished.contains(current) {
            if let Some(&cycle_start) = positions.get(current) {
                for &id in &chain[cycle_start..] {
                    diagnostics.push(EntryRelationIssue {
                        owner: id.into(),
                        kind: EntryRelationIssueKind::Cycle,
                    });
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

fn has_relationship_facts(facts: &EntryFacts) -> bool {
    !facts.names.is_empty()
        || !facts.name_bindings.is_empty()
        || !facts.alias_groups.is_empty()
        || facts.alias_of.is_some()
}

impl<'store> ContentContext<'store> {
    /// Return selectable names only when every explicit binding resolves and
    /// matches the original authored form content.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained binding content does not
    /// resolve in this store.
    pub fn entry_validated_names(
        self,
        owner: EntryOwner<'store>,
    ) -> Result<Option<&'store [String]>, ContentReadError> {
        let Some(_) = valid_name_bindings(self, owner)? else {
            return Ok(None);
        };
        Ok(owner.facts().map(|facts| facts.names.as_slice()))
    }

    /// Return same-owner alias groups only when names and groups are valid.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained binding content does not
    /// resolve in this store.
    pub fn entry_validated_alias_groups(
        self,
        owner: EntryOwner<'store>,
    ) -> Result<Option<&'store [Vec<String>]>, ContentReadError> {
        let Some(bound) = valid_name_bindings(self, owner)? else {
            return Ok(None);
        };
        let Some(facts) = owner.facts() else {
            return Ok(None);
        };
        Ok(groups_are_valid(facts, &bound).then_some(facts.alias_groups.as_slice()))
    }
}

fn valid_name_bindings(
    content: ContentContext<'_>,
    owner: EntryOwner<'_>,
) -> Result<Option<BTreeSet<usize>>, ContentReadError> {
    let Some(facts) = owner.facts() else {
        return Ok(None);
    };
    if content.entry_forms(owner)?.is_none() {
        return Ok(None);
    }
    let mut names = BTreeSet::new();
    for binding in &facts.name_bindings {
        let Some(expected) = facts.names.get(binding.name) else {
            return Ok(None);
        };
        if !names.insert(binding.name) || binding.occurrences.is_empty() {
            return Ok(None);
        }
        for occurrence in &binding.occurrences {
            if occurrence
                .parts
                .iter()
                .any(|part| !inside_head(facts, part))
            {
                return Ok(None);
            }
            if !content.entry_form_text_equals(owner, occurrence, expected)? {
                return Ok(None);
            }
        }
    }
    Ok((names.len() == facts.names.len()).then_some(names))
}

fn inside_head(facts: &EntryFacts, piece: &EntryContentSlice) -> bool {
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
                        NameCase::Sensitive => *candidate == name,
                        NameCase::Insensitive => candidate.eq_ignore_ascii_case(name),
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
        Block, DefinitionItem, DocumentMeta, EntryForm, EntryInlineRoot, EntryKind,
        EntryNameBinding, EntryNameEvidence, LayoutHint, SourceCoordinates, SourceFormat,
        SourceIdentity, SourceKey, SourceRecord,
    };

    fn entry(
        fixture: &mut crate::test_support::ContentFixture,
        id: &str,
        names: &[&str],
    ) -> DefinitionItem {
        DefinitionItem {
            source: None,
            entry: Some(EntryFacts {
                id: id.into(),
                kind: EntryKind::Parameter {
                    parameter_kind: crate::ParameterKind::Option,
                },
                case: NameCase::Sensitive,
                names: names.iter().map(|name| (*name).into()).collect(),
                value_domain: None,
                forms: (0..names.len()).map(EntryForm::term).collect(),
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
            terms: names.iter().map(|name| vec![fixture.code(*name)]).collect(),
            description: vec![Block::Paragraph {
                children: vec![fixture.text("Body --hidden")],
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: crate::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
        }
    }

    fn document(content_store: crate::ContentStore, items: Vec<DefinitionItem>) -> Document {
        Document {
            heading: None,
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "test".to_owned(),
                },
                format: SourceFormat::Markdown,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            content_store,
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            sections: Vec::new(),
            blocks: vec![Block::DefinitionList {
                declaration_groups: Vec::new(),
                items,
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
        }
    }

    fn codes(
        fixture: &crate::test_support::ContentFixture,
        items: Vec<DefinitionItem>,
    ) -> Vec<String> {
        crate::validate_document(&document(fixture.store().clone(), items))
            .into_iter()
            .filter_map(|d| d.code)
            .collect()
    }

    #[test]
    fn validation_snapshot_keeps_duplicate_and_relation_findings_together() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let mut invalid = entry(&mut fixture, "duplicate", &["-a"]);
        invalid.entry.as_mut().unwrap().alias_of = Some("missing".into());
        let duplicate = entry(&mut fixture, "duplicate", &["-b"]);
        let doc = document(fixture.store().clone(), vec![invalid, duplicate]);
        let snapshot = crate::DocumentValidation::new(&doc);
        assert!(std::ptr::eq(snapshot.document(), std::ptr::from_ref(&doc)));
        assert_eq!(snapshot.index(), &crate::DocumentIndex::build(&doc));
        assert_eq!(snapshot.relation_issues(), entry_relation_issues(&doc));
        let codes = snapshot
            .diagnostics()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.duplicate-identity"));
        assert!(codes.contains(&"ir.invalid-entry-alias-of"));
        assert_eq!(snapshot.diagnostics(), crate::validate_document(&doc));
    }

    #[test]
    fn typed_issues_and_document_diagnostics_share_one_relation_policy() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let mut item = entry(&mut fixture, "probe", &["-a", "--all"]);
        item.entry.as_mut().unwrap().alias_groups = vec![vec!["-a".into(), "hidden".into()]];
        let doc = document(fixture.store().clone(), vec![item]);
        let issues = entry_relation_issues(&doc);
        assert_eq!(
            issues,
            vec![EntryRelationIssue {
                owner: "probe".into(),
                kind: EntryRelationIssueKind::AliasGroups
            }]
        );
        assert_eq!(
            issues
                .iter()
                .map(EntryRelationIssue::diagnostic)
                .collect::<Vec<_>>(),
            crate::validate_document(&doc)
        );
    }

    #[test]
    fn explicit_groups_preserve_shared_body_without_merging_subjects() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let mut item = entry(
            &mut fixture,
            "time-bounds",
            &["-S", "--since", "-U", "--until"],
        );
        let body = item.description.clone();
        item.entry.as_mut().unwrap().alias_groups = vec![
            vec!["-S".into(), "--since".into()],
            vec!["-U".into(), "--until".into()],
        ];
        let doc = document(fixture.store().clone(), vec![item]);
        assert!(crate::validate_document(&doc).is_empty());
        let decoded: Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, decoded);
        let Block::DefinitionList { items, .. } = &decoded.blocks[0] else {
            panic!("definitions");
        };
        assert_eq!(items[0].description, body);
        assert_eq!(items[0].entry.as_ref().unwrap().alias_groups.len(), 2);
        let mut related = entry(&mut fixture, "related", &["--time"]);
        related.entry.as_mut().unwrap().alias_of = Some("time-bounds".into());
        assert!(
            codes(&fixture, vec![items[0].clone(), related])
                .contains(&"ir.invalid-entry-alias-of".into())
        );
    }

    #[test]
    fn invalid_groups_never_establish_equivalence_or_hidden_names() {
        let mut fixture = crate::test_support::ContentFixture::body();
        for groups in [
            vec![vec!["-S"]],
            vec![vec!["-S", "-S"]],
            vec![vec!["-S", "--hidden"]],
            vec![vec!["-S", "--since"], vec!["--since", "-S"]],
        ] {
            let mut item = entry(&mut fixture, "since", &["-S", "--since"]);
            item.entry.as_mut().unwrap().alias_groups = groups
                .into_iter()
                .map(|g| g.into_iter().map(str::to_owned).collect())
                .collect();
            assert!(codes(&fixture, vec![item]).contains(&"ir.invalid-entry-alias-groups".into()));
        }
        let mut item = entry(&mut fixture, "case", &["-s", "-S"]);
        let facts = item.entry.as_mut().unwrap();
        facts.case = NameCase::Insensitive;
        facts.alias_groups = vec![vec!["-s".into(), "-S".into()]];
        assert!(codes(&fixture, vec![item]).contains(&"ir.invalid-entry-alias-groups".into()));
    }

    #[test]
    fn name_bindings_cannot_address_body_or_claim_different_text() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let mut item = entry(&mut fixture, "since", &["--since"]);
        item.entry.as_mut().unwrap().name_bindings[0].occurrences[0].parts[0].root =
            EntryInlineRoot::Block { index: 0 };
        assert!(codes(&fixture, vec![item]).contains(&"ir.invalid-entry-name-binding".into()));
        let mut item = entry(&mut fixture, "since", &["--since"]);
        item.entry.as_mut().unwrap().names[0] = "--hidden".into();
        assert!(codes(&fixture, vec![item]).contains(&"ir.invalid-entry-name-binding".into()));
    }

    #[test]
    fn forward_relationships_require_unique_compatible_targets_and_no_cycles() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let mut first = entry(&mut fixture, "first", &["--first"]);
        first.entry.as_mut().unwrap().alias_of = Some("second".into());
        let second = entry(&mut fixture, "second", &["--second"]);
        assert!(codes(&fixture, vec![first.clone(), second.clone()]).is_empty());
        assert!(codes(&fixture, vec![first.clone()]).contains(&"ir.invalid-entry-alias-of".into()));
        assert!(
            codes(
                &fixture,
                vec![first.clone(), second.clone(), second.clone()]
            )
            .contains(&"ir.invalid-entry-alias-of".into())
        );
        let mut incompatible = second.clone();
        incompatible.entry.as_mut().unwrap().kind = EntryKind::Command;
        assert!(
            codes(&fixture, vec![first.clone(), incompatible])
                .contains(&"ir.invalid-entry-alias-of".into())
        );
        let mut cyclic = second;
        cyclic.entry.as_mut().unwrap().alias_of = Some("first".into());
        assert_eq!(
            codes(&fixture, vec![first, cyclic])
                .iter()
                .filter(|code| code.as_str() == "ir.cyclic-entry-alias")
                .count(),
            2
        );
        let mut item = entry(&mut fixture, "self", &["--self"]);
        item.entry.as_mut().unwrap().alias_of = Some("self".into());
        assert!(codes(&fixture, vec![item]).contains(&"ir.invalid-entry-alias-of".into()));
    }
}
