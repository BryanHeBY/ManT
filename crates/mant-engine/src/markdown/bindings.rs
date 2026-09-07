//! Original parser identities shared by annotation collection and lowering.
use super::{SpannedEvent, directives::DomainDeclarationState, metadata::MetadataDeclarations};
use mant_ir::SourceSpan;
use pulldown_cmark::{Event, Tag, TagEnd};
use std::collections::{BTreeMap, BTreeSet};

/// Byte position of a list in the unmodified parser event stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct OriginalListId(pub(super) usize);

impl OriginalListId {
    pub(super) fn from_source(source: Option<SourceSpan>) -> Option<Self> {
        source?
            .byte_range
            .and_then(|r| usize::try_from(r.start.get()).ok())
            .map(Self)
    }
}

/// Byte position of an item, never the first surviving content block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct OriginalItemId(pub(super) usize);

/// One source-local binding table. All annotation phases share these identities;
/// masked comments and later semantic IDs cannot change the item association.
#[derive(Debug, Clone, Default)]
pub(super) struct ItemBindings {
    positions: BTreeMap<OriginalListId, Vec<OriginalItemId>>,
    pub(super) domains: BTreeMap<OriginalItemId, DomainDeclarationState>,
    pub(super) incomplete_entry_children: BTreeSet<OriginalItemId>,
    pub(super) metadata: MetadataDeclarations,
    pub(super) declared_items: BTreeSet<OriginalItemId>,
}

impl ItemBindings {
    pub(super) fn collect(events: &[SpannedEvent<'_>]) -> Self {
        let mut bindings = Self::default();
        let mut lists = Vec::new();
        for (event, range) in events {
            match event {
                Event::Start(Tag::List(_)) => lists.push(OriginalListId(range.start)),
                Event::End(TagEnd::List(_)) => {
                    lists.pop();
                }
                Event::Start(Tag::Item) => {
                    if let Some(list) = lists.last() {
                        bindings
                            .positions
                            .entry(*list)
                            .or_default()
                            .push(OriginalItemId(range.start));
                    }
                }
                _ => {}
            }
        }
        bindings
    }

    pub(super) fn items(&self, source: Option<SourceSpan>) -> &[OriginalItemId] {
        OriginalListId::from_source(source)
            .and_then(|list| self.positions.get(&list))
            .map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Sources(Vec<usize>);
    impl<'a> mant_ir::visit::Visit<'a> for Sources {
        fn visit_list_item(&mut self, item: &'a mant_ir::ListItem) {
            let span = item.source.expect("original item span");
            self.0
                .push(usize::try_from(span.byte_range.unwrap().start.get()).unwrap());
            mant_ir::visit::walk_list_item(self, item);
        }
    }
    #[test]
    fn identities_follow_original_markers_not_surviving_content() {
        for newline in ["\n", "\r\n", "\r"] {
            let source =
                "-\n  <!-- mant:entry {} -->\n\n  Head\n\n  - Child\n".replace('\n', newline);
            let events = super::super::source::parser_events(&source);
            let bindings = ItemBindings::collect(&events);
            let lists = bindings.positions.values().collect::<Vec<_>>();
            assert_eq!(lists.len(), 2);
            assert_eq!(lists[0], &[OriginalItemId(0)]);
            assert_eq!(lists[1], &[OriginalItemId(source.find("- Child").unwrap())]);
            assert_ne!(lists[0][0], OriginalItemId(source.find("Head").unwrap()));
            let query = crate::query_markdown_text(&source, None).unwrap();
            let mut actual = Sources(Vec::new());
            mant_ir::visit::Visit::visit_document(&mut actual, query.document.as_ref().unwrap());
            assert_eq!(actual.0, [0, source.find("- Child").unwrap()]);
        }
    }
}
