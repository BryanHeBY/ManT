//! One source-order walk assigns real owners and finite literal support.
use super::{Candidate, LocatedNode, ResolvedContent, is_identity};
use super::{plan::Candidates, preview::LiteralHit};
use mant_ir::{Block, EntryOwner, Section};
use mant_protocol::EvidenceBasis;
use std::collections::HashMap;

pub(super) fn collect<'a>(
    content: &'a ResolvedContent,
    query: &str,
    located: &[LocatedNode<'a>],
    accepted: &[bool],
) -> (Candidates<'a>, Vec<usize>, super::support::SupportIndex<'a>) {
    let mut scan = Scan {
        content: content.document.as_ref().map(mant_ir::Document::content),
        query,
        located,
        owners: located
            .iter()
            .enumerate()
            .filter_map(|(i, node)| {
                accepted[i]
                    .then_some(node)
                    .and_then(super::owner)
                    .map(|owner| (owner_key(owner), i))
            })
            .collect(),
        sections: located
            .iter()
            .enumerate()
            .filter_map(|(i, node)| match node {
                LocatedNode::Section { section, .. } => {
                    Some((std::ptr::from_ref(*section) as usize, i))
                }
                LocatedNode::Entry { .. } => None,
            })
            .collect(),
        candidates: Candidates::default(),
        next_order: 0,
        orders: vec![0; located.len()],
        supports: super::support::SupportIndex::default(),
    };
    if let Some(document) = &content.document {
        let flow = document
            .flow()
            .expect("Fixed rejected by explanation preflight");
        scan.blocks(&flow.blocks, None, None, "root");
        scan.sections(&flow.sections, "sections");
    }
    (scan.candidates, scan.orders, scan.supports)
}

struct Scan<'a, 'b> {
    content: Option<mant_ir::ContentContext<'a>>,
    supports: super::support::SupportIndex<'a>,
    query: &'b str,
    located: &'b [LocatedNode<'a>],
    owners: HashMap<usize, usize>,
    sections: HashMap<usize, usize>,
    candidates: Candidates<'a>,
    next_order: usize,
    orders: Vec<usize>,
}

impl<'a> Scan<'a, '_> {
    fn sections(&mut self, sections: &'a [Section], path: &str) {
        for (ordinal, section) in sections.iter().enumerate() {
            let path = format!("{path}/s{ordinal}");
            let index = self.sections[&(std::ptr::from_ref(section) as usize)];
            self.blocks(&section.blocks, None, Some(index), &path);
            self.sections(&section.children, &path);
        }
    }

    fn enter_owner(&mut self, owner: EntryOwner<'a>, current: Option<usize>) -> Option<usize> {
        let Some(&index) = self.owners.get(&owner_key(owner)) else {
            return current;
        };
        self.orders[index] = self.next_order;
        self.next_order += 1;
        let LocatedNode::Entry { entry, .. } = &self.located[index] else {
            unreachable!("owner location")
        };
        let (matched, mut bases) = super::matches::MatchPlan::collect(
            self.content.expect("entry scan belongs to a document"),
            owner,
            entry.names(),
            self.query,
        );
        if is_identity(&self.located[index], self.query) {
            let mut fields = Vec::new();
            if self.located[index].id() == self.query {
                fields.push(mant_protocol::ExplanationIdentityField::Id);
            }
            if self
                .query
                .parse::<mant_ir::OutlinePath>()
                .is_ok_and(|path| &path == self.located[index].path())
            {
                fields.push(mant_protocol::ExplanationIdentityField::Path);
            }
            bases.push(EvidenceBasis::Identity { fields });
        }
        if !bases.is_empty() {
            self.add_owner(index, bases, Vec::new(), matched);
        }
        Some(index)
    }

    fn add_owner(
        &mut self,
        index: usize,
        bases: Vec<EvidenceBasis>,
        hits: Vec<LiteralHit<'a>>,
        matched: super::matches::MatchPlan,
    ) {
        self.candidates.insert(Candidate {
            order: self.orders[index],
            located: Some(index),
            ordinary: None,
            ordinary_item: None,
            section: None,
            block_path: None,
            source: self.located[index].source(),
            bases,
            matched,
            hits,
        });
    }

    fn blocks(
        &mut self,
        blocks: &'a [Block],
        current: Option<usize>,
        section: Option<usize>,
        path: &str,
    ) {
        for (index, block) in blocks.iter().enumerate() {
            let block_path = format!("{path}/b{index}");
            let order = self.next_order;
            self.next_order += 1;
            match block {
                Block::List { items, .. } => {
                    self.supports.record(
                        self.content.expect("support scan belongs to a document"),
                        block,
                        &block_path,
                        &self.owners,
                    );
                    for (i, item) in items.iter().enumerate() {
                        let owner = self.enter_owner(EntryOwner::List(item), current);
                        self.blocks(&item.blocks, owner, section, &format!("{block_path}/i{i}"));
                    }
                }
                Block::DefinitionList { items, .. } => {
                    self.supports.record(
                        self.content.expect("support scan belongs to a document"),
                        block,
                        &block_path,
                        &self.owners,
                    );
                    for (i, item) in items.iter().enumerate() {
                        let accepted = self
                            .owners
                            .contains_key(&owner_key(EntryOwner::Definition(item)));
                        let owner = self.enter_owner(EntryOwner::Definition(item), current);
                        // Accepted HEADs already have direct Name/Form evidence.
                        // A non-entry HEAD remains original visible content;
                        // scan it as literal support without inferring a name.
                        if !accepted {
                            self.definition_terms(block, i, owner, section, &block_path);
                        }
                        self.blocks(
                            &item.description,
                            owner,
                            section,
                            &format!("{block_path}/d{i}"),
                        );
                    }
                }
                Block::Table { rows, .. } => {
                    for (r, row) in rows.iter().enumerate() {
                        for (c, cell) in row.cells.iter().enumerate() {
                            self.blocks(
                                &cell.blocks,
                                current,
                                section,
                                &format!("{block_path}/r{r}/c{c}"),
                            );
                        }
                    }
                }
                _ => {
                    let Some(text) = super::literal::block_text(
                        self.content.expect("block scan belongs to a document"),
                        block,
                    ) else {
                        continue;
                    };
                    let Some(range) = super::literal::first_match(&text, self.query) else {
                        continue;
                    };
                    let hit = LiteralHit {
                        block,
                        term: None,
                        path: block_path.clone(),
                        range,
                    };
                    if let Some(owner) = current {
                        self.add_owner(
                            owner,
                            vec![EvidenceBasis::Literal],
                            vec![hit],
                            super::matches::MatchPlan::default(),
                        );
                    } else {
                        self.candidates.insert(Candidate {
                            order,
                            located: None,
                            ordinary: Some(block),
                            ordinary_item: None,
                            section,
                            block_path: Some(block_path),
                            source: mant_ir::geometry::block_source(block),
                            bases: vec![EvidenceBasis::Literal],
                            matched: super::matches::MatchPlan::default(),
                            hits: vec![hit],
                        });
                    }
                }
            }
        }
    }

    fn definition_terms(
        &mut self,
        block: &'a Block,
        item_index: usize,
        current: Option<usize>,
        section: Option<usize>,
        block_path: &str,
    ) {
        let Block::DefinitionList { items, .. } = block else {
            unreachable!("definition term belongs to a definition list")
        };
        let item = &items[item_index];
        for (term_index, term) in item.terms.iter().enumerate() {
            let content = self.content.expect("term scan belongs to a document");
            let Some(text) = mant_protocol::ExplanationTextRoot::Inline(term).safe_text(content)
            else {
                // A hand-built Flow document can retain a malformed optional
                // term. It is not usable literal evidence for this request.
                continue;
            };
            let Some(range) = super::literal::first_match(&text, self.query) else {
                continue;
            };
            let order = self.next_order;
            self.next_order += 1;
            let hit = LiteralHit {
                block,
                term: Some((item_index, term_index)),
                path: block_path.to_owned(),
                range,
            };
            if let Some(owner) = current {
                self.add_owner(
                    owner,
                    vec![EvidenceBasis::Literal],
                    vec![hit],
                    super::matches::MatchPlan::default(),
                );
            } else {
                self.candidates.insert(Candidate {
                    order,
                    located: None,
                    ordinary: Some(block),
                    ordinary_item: Some(item_index),
                    section,
                    block_path: Some(block_path.to_owned()),
                    source: item.source,
                    bases: vec![EvidenceBasis::Literal],
                    matched: super::matches::MatchPlan::default(),
                    hits: vec![hit],
                });
            }
            // One physical definition item is one ordinary context candidate;
            // additional matched terms do not manufacture another owner.
            break;
        }
    }
}

fn owner_key(owner: EntryOwner<'_>) -> usize {
    match owner {
        EntryOwner::List(item) => std::ptr::from_ref(item) as usize,
        EntryOwner::Definition(item) => std::ptr::from_ref(item) as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::{
        ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, DefinitionItem,
        DefinitionLayout, Document, DocumentBody, DocumentMeta, EntryFacts, EntryForm, EntryKind,
        EntryNameBinding, EntryNameEvidence, FlowBody, Inline, LayoutHint, NameCase, Provenance,
        ResolvedContent, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord,
    };
    use mant_protocol::{EvidenceClass, ExplanationContent, ExplanationContentRange};

    fn term(store: &mut ContentStoreBuilder, text: &str) -> Vec<Inline> {
        let owner = store.push_owner(ContentOwnerKind::DefinitionItem, Provenance::Unknown);
        let root = store.push_root(owner, ContentRootKind::Term, Provenance::Unknown);
        vec![Inline::Text {
            content: store.push_text(
                root,
                text.to_owned(),
                None,
                ContentStyle::default(),
                None,
                None,
                Provenance::Unknown,
            ),
        }]
    }

    fn definition(
        terms: Vec<Vec<Inline>>,
        entry: Option<EntryFacts>,
        description: Vec<Block>,
    ) -> DefinitionItem {
        DefinitionItem {
            source: None,
            entry,
            terms,
            description,
            layout: DefinitionLayout::default(),
        }
    }

    fn list(items: Vec<DefinitionItem>) -> Block {
        Block::DefinitionList {
            items,
            declaration_groups: Vec::new(),
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }
    }

    fn facts(name: &str, id: &str) -> EntryFacts {
        EntryFacts {
            name_bindings: vec![EntryNameBinding {
                name: 0,
                occurrences: vec![EntryForm::term(0)],
                evidence: EntryNameEvidence::Declared,
            }],
            alias_groups: Vec::new(),
            alias_of: None,
            forms: vec![EntryForm::term(0)],
            id: id.into(),
            kind: EntryKind::Term,
            case: NameCase::Sensitive,
            names: vec![name.into()],
            value_domain: None,
        }
    }

    fn flow_content(store: ContentStoreBuilder, blocks: Vec<Block>) -> ResolvedContent {
        ResolvedContent {
            label: "unclassified heads".into(),
            address: None,
            document: Some(Document {
                parser: None,
                sources: vec![SourceRecord {
                    key: SourceKey::FIRST,
                    identity: SourceIdentity::Anonymous {
                        name: "synthetic-heads".into(),
                    },
                    format: SourceFormat::Man,
                    decoded_byte_length: 0,
                    content_sha256: None,
                    coordinates: SourceCoordinates::DecodedUtf8Bytes,
                }],
                root_source: SourceKey::FIRST,
                body: DocumentBody::Flow(FlowBody {
                    content_store: store.finish(),
                    heading: None,
                    blocks,
                    sections: Vec::new(),
                }),
                meta: DocumentMeta::default(),
                fragment_aliases: Vec::new(),
                diagnostics: Vec::new(),
            }),
            tldr: None,
        }
    }

    fn unclassified_content() -> ResolvedContent {
        let mut store = ContentStoreBuilder::new();
        let unused = term(&mut store, "unrelated");
        let root_template = term(&mut store, "FILE_TEMPLATE_*");
        let outer_term = term(&mut store, "outer");
        let inner_template = term(&mut store, "FILE_TEMPLATE_*");
        let nested = list(vec![definition(vec![inner_template], None, Vec::new())]);
        let outer = definition(
            vec![outer_term],
            Some(facts("outer", "outer")),
            vec![nested],
        );
        flow_content(
            store,
            vec![
                list(vec![
                    definition(vec![unused], None, Vec::new()),
                    definition(vec![root_template], None, Vec::new()),
                ]),
                list(vec![outer]),
            ],
        )
    }

    #[test]
    fn unclassified_terms_remain_bounded_literal_mentions_with_original_coordinates() {
        // The exact EN00 terms.1 and a nested .Bl -tag/.It FILE_TEMPLATE_*
        // input ran pinned CVS -Tutf8 first. man_term.c::pre_TP and
        // mdoc_term.c::termp_it_pre retain these visible HEAD glyphs; ManT's
        // optional classification must not erase their ordinary text.
        let content = unclassified_content();
        let result = super::super::select_explanation(&content, "FILE_TEMPLATE_*").unwrap();
        assert_eq!(result.counts.direct_entry.total, 0);
        assert_eq!(result.counts.entry_mention.total, 1);
        assert_eq!(result.counts.context_mention.total, 1);
        let nested = result
            .evidence
            .iter()
            .find(|record| record.class == EvidenceClass::EntryMention)
            .unwrap();
        assert_eq!(nested.outline.node.id(), "outer");
        assert!(matches!(
            nested.previews[0].content_ranges.as_slice(),
            [ExplanationContentRange::DefinitionTerm { path, item_index: 0, term_index: 0, .. }]
                if !path.is_empty()
        ));
        let ordinary = result
            .evidence
            .iter()
            .find(|record| record.class == EvidenceClass::ContextMention)
            .unwrap();
        assert_eq!(ordinary.previews[0].text, "FILE_TEMPLATE_*");
        assert!(matches!(
            ordinary.previews[0].content_ranges.as_slice(),
            [ExplanationContentRange::DefinitionTerm { path, item_index: 0, term_index: 0, start_char: 0, end_char: 15 }]
                if path.is_empty()
        ));
        let Some(ExplanationContent::Block {
            block: block @ Block::DefinitionList { items, .. },
        }) = &ordinary.content
        else {
            panic!("one ordinary definition item was budgeted and copied")
        };
        assert_eq!(items.len(), 1);
        let projected = result
            .content_projection
            .as_ref()
            .expect("returned term has a response-local content store")
            .content();
        assert_eq!(
            projected.plain_text(&items[0].terms[0]).unwrap(),
            "FILE_TEMPLATE_*"
        );
        assert_eq!(
            ordinary.previews[0].content_ranges[0]
                .resolve(projected, block)
                .unwrap()
                .safe_text(projected)
                .unwrap(),
            "FILE_TEMPLATE_*"
        );
        result.validate_references().unwrap();
        assert_eq!(
            super::super::select_explanation(&content, "FILE_TEMPLATE_1")
                .unwrap()
                .total,
            0
        );
        let direct = super::super::select_explanation(&content, "outer").unwrap();
        assert_eq!(direct.counts.direct_entry.total, 1);
        assert_eq!(direct.counts.context_mention.total, 0);
    }

    #[test]
    fn rejected_name_binding_keeps_head_text_without_direct_entry_evidence() {
        // This is an intentionally damaged optional fact, not a new roff
        // behavior assertion. Original term text remains independently safe.
        let mut store = ContentStoreBuilder::new();
        let bad_term = term(&mut store, "broken");
        let good_term = term(&mut store, "good");
        let form_only_term = term(&mut store, "form-only");
        let mut bad = facts("broken", "broken-id");
        bad.name_bindings[0].name = 1; // Invalid index, while its form still resolves.
        let mut form_only = facts("unused", "form-only-id");
        form_only.names.clear();
        form_only.name_bindings.clear();
        let content = flow_content(
            store,
            vec![list(vec![
                definition(vec![bad_term], Some(bad), Vec::new()),
                definition(vec![good_term], Some(facts("good", "good-id")), Vec::new()),
                definition(vec![form_only_term], Some(form_only), Vec::new()),
            ])],
        );
        let document: Document = serde_json::from_str(
            &serde_json::to_string(content.document.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        let content = ResolvedContent {
            document: Some(document),
            ..content
        };

        let broken = super::super::select_explanation(&content, "broken").unwrap();
        assert_eq!(broken.counts.direct_entry.total, 0);
        assert_eq!(broken.counts.context_mention.total, 1);
        assert_eq!(broken.evidence[0].previews[0].text, "broken");
        broken.validate_references().unwrap();

        let good = super::super::select_explanation(&content, "good").unwrap();
        assert_eq!(good.counts.direct_entry.total, 1);
        good.validate_references().unwrap();

        // Some(empty) is a valid form-only entry, distinct from None for the
        // damaged binding above.
        let form_only = super::super::select_explanation(&content, "form-only").unwrap();
        assert_eq!(form_only.counts.direct_entry.total, 1);
        form_only.validate_references().unwrap();
    }

    #[test]
    fn rejected_nested_entry_head_belongs_to_nearest_accepted_ancestor() {
        let mut store = ContentStoreBuilder::new();
        let outer_term = term(&mut store, "outer");
        let bad_term = term(&mut store, "broken");
        let mut bad = facts("broken", "broken-id");
        bad.name_bindings[0].name = 1;
        let nested = list(vec![definition(vec![bad_term], Some(bad), Vec::new())]);
        let outer = definition(
            vec![outer_term],
            Some(facts("outer", "outer-id")),
            vec![nested],
        );
        let content = flow_content(store, vec![list(vec![outer])]);

        let result = super::super::select_explanation(&content, "broken").unwrap();
        assert_eq!(result.counts.direct_entry.total, 0);
        assert_eq!(result.counts.entry_mention.total, 1);
        assert_eq!(result.counts.context_mention.total, 0);
        assert_eq!(result.evidence[0].outline.node.id(), "outer-id");
        assert_eq!(result.evidence[0].previews[0].text, "broken");
        result.validate_references().unwrap();
    }
}
