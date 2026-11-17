//! Join native and IR owners by source coordinate and occurrence.
use super::source::{body, readable};
use libmandoc_rs::{Node, NodeKind};
use mant_ir::{
    Block, DefinitionItem, Document, SourceSpan,
    visit::{self, Visit},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type OwnerKey = (u32, u32, usize);

#[derive(Default)]
pub(super) struct ObservedOwners<'a> {
    pub(super) owners: BTreeMap<OwnerKey, &'a DefinitionItem>,
    pointers: BTreeMap<usize, OwnerKey>,
    occurrences: BTreeMap<(u32, u32), usize>,
    pub(super) list_owners: BTreeSet<OwnerKey>,
    group_pointers: Vec<Vec<usize>>,
    pub(super) groups: Vec<Vec<OwnerKey>>,
    pub(super) invalid: usize,
}
impl<'a> Visit<'a> for ObservedOwners<'a> {
    fn visit_list_item(&mut self, item: &'a mant_ir::ListItem) {
        // Ordinal/bullet conversion changes the block kind, not its place in
        // a repeated source-coordinate stream.
        if let Some(source) = item.source {
            let coordinate = key(source);
            let occurrence = self.occurrences.entry(coordinate).or_default();
            self.list_owners
                .insert((coordinate.0, coordinate.1, *occurrence));
            *occurrence += 1;
        }
        visit::walk_list_item(self, item);
    }
    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        if let Some(source) = item.source {
            let coordinate = key(source);
            let occurrence = self.occurrences.entry(coordinate).or_default();
            let key = (coordinate.0, coordinate.1, *occurrence);
            *occurrence += 1;
            self.owners.insert(key, item);
            self.pointers.insert(std::ptr::from_ref(item) as usize, key);
        }
        visit::walk_definition_item(self, item);
    }
    fn visit_block(&mut self, block: &'a Block) {
        if let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = block
        {
            for group in declaration_groups {
                let sources = group.resolve(items).and_then(|members| {
                    members
                        .iter()
                        .map(|i| i.source.map(|_| std::ptr::from_ref(i) as usize))
                        .collect::<Option<Vec<_>>>()
                });
                if let Some(sources) = sources {
                    self.group_pointers.push(sources);
                } else {
                    self.invalid += 1;
                }
            }
        }
        visit::walk_block(self, block);
    }
}
fn key(source: SourceSpan) -> (u32, u32) {
    (source.line, source.column)
}

impl<'a> ObservedOwners<'a> {
    pub(super) fn collect(document: &'a Document) -> Self {
        let mut owners = Self::default();
        owners.visit_document(document);
        owners.groups = owners
            .group_pointers
            .iter()
            .map(|group| group.iter().map(|p| owners.pointers[p]).collect())
            .collect();
        owners
    }
}

pub(super) struct NativeOwners<'a> {
    by_node: BTreeMap<usize, OwnerKey>,
    by_key: BTreeMap<OwnerKey, &'a Node>,
}

impl<'a> NativeOwners<'a> {
    pub(super) fn collect(root: &'a Node, list_owners: &BTreeSet<OwnerKey>) -> Self {
        let mut owners = Self {
            by_node: BTreeMap::new(),
            by_key: BTreeMap::new(),
        };
        native_keys(
            root,
            &mut BTreeMap::new(),
            &mut owners.by_node,
            &mut owners.by_key,
            list_owners,
        );
        owners
    }

    pub(super) fn key(&self, node: &Node) -> Option<OwnerKey> {
        self.by_node
            .get(&(std::ptr::from_ref(node) as usize))
            .copied()
    }

    pub(super) fn node(&self, key: &OwnerKey) -> Option<&'a Node> {
        self.by_key.get(key).copied()
    }
}

// Physical source coordinates can repeat during macro expansion. Preserve
// every occurrence in syntax/IR traversal order instead of overwriting a
// coordinate bucket. Deleted/reordered owners leave unmatched obligations.
fn native_keys<'a>(
    node: &'a Node,
    counts: &mut BTreeMap<(u32, u32), usize>,
    keys: &mut BTreeMap<usize, OwnerKey>,
    source_nodes: &mut BTreeMap<OwnerKey, &'a Node>,
    list_owners: &BTreeSet<OwnerKey>,
) {
    let mut previous: Option<(&Node, OwnerKey)> = None;
    for child in &node.children {
        if child.kind == NodeKind::Block
            && matches!(
                child.macro_token.as_deref(),
                Some("IP" | "TP" | "It" | "TQ")
            )
        {
            // IP's first argument is its tag; later HEAD children are
            // layout widths, not term text (`.IP "" 4`).
            let headless = child
                .children
                .iter()
                .find(|n| n.kind == NodeKind::Head)
                .and_then(|head| head.children.first())
                .is_none_or(|tag| !readable(tag));
            let continued = previous.filter(|(before, _)| before.flow_epoch == child.flow_epoch);
            let merged = child.macro_token.as_deref() == Some("TQ")
                && continued.is_some_and(|(before, _)| !body(before));
            let continuation = child.macro_token.as_deref() == Some("IP")
                && headless
                && continued
                    .is_some_and(|(before, key)| body(before) && !list_owners.contains(&key));
            if continuation {
                // Headless IP is another paragraph of the previous owner,
                // not a declaration in a source run.
                native_keys(child, counts, keys, source_nodes, list_owners);
                continue;
            }
            let source = if merged {
                continued.expect("checked preceding owner").1
            } else {
                let coordinate = (child.line, child.column);
                let occurrence = counts.entry(coordinate).or_default();
                let source = (coordinate.0, coordinate.1, *occurrence);
                *occurrence += 1;
                source
            };
            keys.insert(std::ptr::from_ref(child) as usize, source);
            source_nodes.insert(source, child);
            previous = Some((child, source));
        } else if !matches!(
            child.macro_token.as_deref(),
            Some("PD" | "Sm" | "Tg" | "ft")
        ) {
            previous = None;
        }
        native_keys(child, counts, keys, source_nodes, list_owners);
    }
}
