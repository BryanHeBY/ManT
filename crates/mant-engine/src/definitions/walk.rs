//! Source-order semantic coordinates over unchanged native and Markdown owners.
use mant_ir::{Block, EntryOwner, SourceSpan};

/// One identified owner, retaining enough context to excerpt its original item.
pub(crate) struct ContentEntry<'a> {
    pub(crate) item: EntryOwner<'a>,
    /// Validated once for this immutable location snapshot, never raw facts.
    pub(crate) names: &'a [String],
    pub(crate) source: Option<SourceSpan>,
    pub(crate) indices: Vec<usize>,
    pub(crate) ancestors: Vec<EntryOwner<'a>>,
    container: &'a Block,
    item_index: usize,
}

impl ContentEntry<'_> {
    /// Copy only the selected owner, not its siblings or a synthetic definition.
    pub(crate) fn content(&self) -> Block {
        match self.container {
            Block::List {
                kind,
                compact,
                items,
                layout,
                source,
            } => Block::List {
                kind: kind.for_excerpt(self.item_index),
                compact: *compact,
                items: vec![items[self.item_index].clone()],
                layout: *layout,
                source: *source,
            },
            Block::DefinitionList {
                items,
                compact,
                layout,
                source,
            } => Block::DefinitionList {
                items: vec![items[self.item_index].clone()],
                compact: *compact,
                layout: *layout,
                source: *source,
            },
            _ => unreachable!("entry containers are lists"),
        }
    }
}

/// Borrowed serialization of exactly the same single-owner block as `content()`.
/// Copy-budget checks can measure it without first cloning a large description.
impl serde::Serialize for ContentEntry<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;
        let mut output = serializer.serialize_map(None)?;
        let (layout, source) = match self.container {
            Block::List {
                kind,
                compact,
                items,
                layout,
                source,
            } => {
                output.serialize_entry("type", "list")?;
                output.serialize_entry("kind", &kind.for_excerpt(self.item_index))?;
                if *compact {
                    output.serialize_entry("compact", compact)?;
                }
                output.serialize_entry("items", std::slice::from_ref(&items[self.item_index]))?;
                (layout, source)
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                source,
            } => {
                output.serialize_entry("type", "definition-list")?;
                output.serialize_entry("items", std::slice::from_ref(&items[self.item_index]))?;
                if *compact {
                    output.serialize_entry("compact", compact)?;
                }
                (layout, source)
            }
            _ => unreachable!("entry containers are lists"),
        };
        if !layout.is_empty() {
            output.serialize_entry("layout", layout)?;
        }
        if let Some(source) = source {
            output.serialize_entry("source", source)?;
        }
        output.end()
    }
}

/// Same semantic pre-order as `SemanticIndex`, with source presentation retained.
pub(crate) fn content_entries(blocks: &[Block]) -> Vec<ContentEntry<'_>> {
    let mut entries = Vec::new();
    collect_scope(blocks, &[], &mut Vec::new(), &mut entries);
    entries
}

fn collect_scope<'a>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<EntryOwner<'a>>,
    output: &mut Vec<ContentEntry<'a>>,
) {
    collect_direct(blocks, parent_indices, ancestors, &mut 0, output);
}

fn collect_direct<'a>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<EntryOwner<'a>>,
    direct_index: &mut usize,
    output: &mut Vec<ContentEntry<'a>>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    if item.entry.is_some() {
                        collect_owner(
                            block,
                            index,
                            EntryOwner::List(item),
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    } else {
                        collect_direct(
                            &item.blocks,
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    }
                }
            }
            Block::DefinitionList { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    if item.entry.is_some() {
                        collect_owner(
                            block,
                            index,
                            EntryOwner::Definition(item),
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    } else {
                        collect_direct(
                            &item.description,
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    }
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_direct(
                        &cell.blocks,
                        parent_indices,
                        ancestors,
                        direct_index,
                        output,
                    );
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_owner<'a>(
    container: &'a Block,
    item_index: usize,
    item: EntryOwner<'a>,
    parent_indices: &[usize],
    ancestors: &mut Vec<EntryOwner<'a>>,
    direct_index: &mut usize,
    output: &mut Vec<ContentEntry<'a>>,
) {
    *direct_index += 1;
    let mut indices = parent_indices.to_vec();
    indices.push(*direct_index);
    output.push(ContentEntry {
        item,
        names: item.validated_names().unwrap_or_default(),
        source: item.source(),
        indices: indices.clone(),
        ancestors: ancestors.clone(),
        container,
        item_index,
    });
    ancestors.push(item);
    collect_scope(item.blocks(), &indices, ancestors, output);
    ancestors.pop();
}

#[cfg(test)]
mod budget_tests {
    #[test]
    fn borrowed_budget_shape_equals_the_owned_original_excerpt() {
        // Unit tests ship with src; repository integration fixtures do not.
        let content = crate::query_roff_bytes(
            b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -a\nFirst.\n.TP\n.B -b\nSecond.\n",
        )
        .unwrap();
        let document = content.document.unwrap();
        let mut checked = 0;
        for section in &document.sections {
            for entry in super::content_entries(&section.blocks) {
                assert_eq!(
                    serde_json::to_value(&entry).unwrap(),
                    serde_json::to_value(entry.content()).unwrap()
                );
                checked += 1;
            }
        }
        assert!(checked >= 2);
    }
}
