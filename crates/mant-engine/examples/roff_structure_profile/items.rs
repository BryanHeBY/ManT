//! Optional, bounded item census for structural review; never an acceptance rule.
//!
//! Native IDs and flow generations are parse-local. Source coordinates only
//! select diagnostic candidates: duplicate/unknown origins stay ambiguous. The
//! caller retains the exact input identity and compares its real owner records.
use std::collections::BTreeMap;

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{Block, Document, Inline, Section, SourceSpan};
use serde::Serialize;

const MAX_ITEMS: usize = 32_768;
const MAX_WITNESS: usize = 96;
const MAX_ANCESTORS: usize = 8;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ItemCensus {
    native: Vec<NativeItem>,
    ir: Vec<IrItem>,
    native_total: usize,
    ir_total: usize,
    native_node_visits: usize,
    ir_node_visits: usize,
    native_omitted: usize,
    ir_omitted: usize,
    /// Indices locate witnesses, not proof of loss: category migration and
    /// merged item ownership need exact source/ancestry review.
    unmatched_native: Vec<usize>,
    unmatched_ir: Vec<usize>,
    ambiguous_origins: Vec<Origin>,
}

#[derive(Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
struct Origin {
    line: u32,
    column: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeItem {
    id: u32,
    macro_name: String,
    origin: Origin,
    flow_epoch: usize,
    ancestor_ids: Vec<u32>,
    counted: bool,
    head: String,
    body: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IrItem {
    kind: &'static str,
    origin: Origin,
    container_path: Vec<String>,
    entry_id: Option<String>,
    head: String,
    body: String,
}

#[derive(Clone, Copy)]
enum Part {
    Head,
    Body,
}

pub(super) fn item_census(native: &Node, document: &Document) -> ItemCensus {
    let mut result = ItemCensus::default();
    walk_native(
        native,
        None,
        Part::Body,
        false,
        &mut Vec::new(),
        &mut result,
    );
    walk_ir_blocks(
        &document.blocks,
        None,
        &mut vec!["preamble".to_owned()],
        &mut result,
    );
    for (index, section) in document.sections.iter().enumerate() {
        walk_ir_section(section, &mut vec![format!("section:{index}")], &mut result);
    }
    result.native_omitted = result.native_total - result.native.len();
    result.ir_omitted = result.ir_total - result.ir.len();
    compare_origins(&mut result);
    result
}

fn native_item(node: &Node) -> bool {
    node.kind == NodeKind::Block && matches!(node.macro_token.as_deref(), Some("IP" | "TP" | "It"))
}

fn walk_native(
    node: &Node,
    inherited: Option<usize>,
    inherited_part: Part,
    inherited_column_list: bool,
    ancestors: &mut Vec<u32>,
    census: &mut ItemCensus,
) -> bool {
    census.native_node_visits += 1;
    let column_list = if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("Bl") {
        super::native::mdoc_list_topology_kind(node)
            == Some(super::native::MdocContainerKind::Table)
    } else {
        inherited_column_list
    };
    let owner = if native_item(node) {
        census.native_total += 1;
        if census.native.len() < MAX_ITEMS {
            let index = census.native.len();
            census.native.push(NativeItem {
                id: node.id,
                macro_name: node.macro_token.as_deref().unwrap_or_default().to_owned(),
                origin: Origin {
                    line: node.line,
                    column: node.column,
                },
                flow_epoch: node.flow_epoch,
                ancestor_ids: ancestors
                    .iter()
                    .rev()
                    .take(MAX_ANCESTORS)
                    .copied()
                    .collect(),
                counted: node.macro_token.as_deref() == Some("It") && !column_list
                    || super::native::ast_tag_is_bullet(node),
                head: String::new(),
                body: String::new(),
            });
            Some(index)
        } else {
            // Do not append an omitted nested item's witness to its parent.
            None
        }
    } else {
        inherited
    };
    let part = match node.kind {
        NodeKind::Head => Part::Head,
        NodeKind::Body => Part::Body,
        _ => inherited_part,
    };
    if let Some(index) = owner
        && node.kind == NodeKind::Text
        && !node.flags.no_print
        && let Some(text) = &node.text
    {
        let item = &mut census.native[index];
        append_witness(
            match part {
                Part::Head => &mut item.head,
                Part::Body => &mut item.body,
            },
            text,
        );
    }
    ancestors.push(node.id);
    let mut child_visible = false;
    let mut body_visible = false;
    for child in &node.children {
        let visible = walk_native(child, owner, part, column_list, ancestors, census);
        child_visible |= visible;
        body_visible |= child.kind == NodeKind::Body && visible;
    }
    ancestors.pop();
    if native_item(node)
        && !column_list
        && body_visible
        && let Some(index) = owner
    {
        census.native[index].counted = true;
    }
    !node.flags.no_print
        && node.kind != NodeKind::Comment
        && (node.text.as_deref().is_some_and(|text| !text.is_empty()) || child_visible)
}

fn append_witness(output: &mut String, value: &str) {
    // The cap is bytes, not a re-count of the cumulative witness per leaf.
    // Never truncate inside a UTF-8 scalar. Spaces separate raw leaf witnesses;
    // this is diagnostic source evidence, not a simulated native word stream.
    if output.len() < MAX_WITNESS && !output.is_empty() && !value.is_empty() {
        output.push(' ');
    }
    for character in value.chars() {
        if output.len() + character.len_utf8() > MAX_WITNESS {
            break;
        }
        output.push(character);
    }
}

fn walk_ir_section(section: &Section, path: &mut Vec<String>, census: &mut ItemCensus) {
    walk_ir_blocks(&section.blocks, None, path, census);
    for (index, child) in section.children.iter().enumerate() {
        path.push(format!("section:{index}"));
        walk_ir_section(child, path, census);
        path.pop();
    }
}

fn ir_owner(
    kind: &'static str,
    source: Option<SourceSpan>,
    entry: Option<&mant_ir::EntryFacts>,
    path: &[String],
    census: &mut ItemCensus,
) -> Option<usize> {
    census.ir_total += 1;
    if census.ir.len() == MAX_ITEMS {
        return None;
    }
    let index = census.ir.len();
    census.ir.push(IrItem {
        kind,
        origin: Origin {
            line: source.map_or(0, |span| span.line),
            column: source.map_or(0, |span| span.column),
        },
        container_path: path.iter().take(64).cloned().collect(),
        entry_id: entry.map(|facts| facts.id.to_string()),
        head: String::new(),
        body: String::new(),
    });
    Some(index)
}

fn walk_ir_blocks(
    blocks: &[Block],
    owner: Option<usize>,
    path: &mut Vec<String>,
    census: &mut ItemCensus,
) {
    for (index, block) in blocks.iter().enumerate() {
        census.ir_node_visits += 1;
        path.push(format!("block:{index}"));
        match block {
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    path.push(format!("item:{item_index}"));
                    let item_owner =
                        ir_owner("generic", item.source, item.entry.as_ref(), path, census);
                    walk_ir_blocks(&item.blocks, item_owner, path, census);
                    path.pop();
                }
            }
            Block::DefinitionList { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    path.push(format!("item:{item_index}"));
                    let item_owner =
                        ir_owner("definition", item.source, item.entry.as_ref(), path, census);
                    for term in &item.terms {
                        walk_ir_inlines(term, item_owner, Part::Head, census);
                    }
                    walk_ir_blocks(&item.description, item_owner, path, census);
                    path.pop();
                }
            }
            Block::Table { rows, .. } => {
                for (row_index, row) in rows.iter().enumerate() {
                    path.push(format!("row:{row_index}"));
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        path.push(format!("cell:{cell_index}"));
                        // A table contains its own owners; unrelated table
                        // text cannot become a list item's body witness.
                        walk_ir_blocks(&cell.blocks, None, path, census);
                        path.pop();
                    }
                    path.pop();
                }
            }
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                walk_ir_inlines(children, owner, Part::Body, census);
            }
            Block::Unsupported { text, .. } | Block::Equation { value: text, .. } => {
                if let Some(index) = owner {
                    append_witness(&mut census.ir[index].body, text);
                }
            }
            Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => {}
        }
        path.pop();
    }
}

fn walk_ir_inlines(inlines: &[Inline], owner: Option<usize>, part: Part, census: &mut ItemCensus) {
    for inline in inlines {
        census.ir_node_visits += 1;
        match inline {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => walk_ir_inlines(children, owner, part, census),
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                if let Some(index) = owner {
                    let item = &mut census.ir[index];
                    append_witness(
                        match part {
                            Part::Head => &mut item.head,
                            Part::Body => &mut item.body,
                        },
                        value,
                    );
                }
            }
            Inline::LineBreak { .. } | Inline::Anchor { .. } => {}
        }
    }
}

fn compare_origins(census: &mut ItemCensus) {
    let mut origins = BTreeMap::<Origin, (Vec<usize>, Vec<usize>)>::new();
    for (index, item) in census.native.iter().enumerate() {
        origins.entry(item.origin).or_default().0.push(index);
    }
    for (index, item) in census.ir.iter().enumerate() {
        origins.entry(item.origin).or_default().1.push(index);
    }
    for (origin, (native, ir)) in origins {
        if origin.line == 0 || native.len() > 1 || ir.len() > 1 {
            census.ambiguous_origins.push(origin);
        } else if ir.is_empty() {
            census.unmatched_native.extend(native);
        } else if native.is_empty() {
            census.unmatched_ir.extend(ir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_duplicate_and_unknown_origins_do_not_become_acceptance() {
        let mut census = ItemCensus::default();
        for origin in [
            Origin { line: 0, column: 0 },
            Origin { line: 2, column: 2 },
            Origin { line: 2, column: 2 },
            Origin { line: 3, column: 2 },
        ] {
            census.native.push(NativeItem {
                id: 0,
                macro_name: "IP".into(),
                origin,
                flow_epoch: 0,
                ancestor_ids: Vec::new(),
                counted: true,
                head: String::new(),
                body: String::new(),
            });
        }
        compare_origins(&mut census);
        assert_eq!(census.unmatched_native, [3]);
        assert_eq!(census.ambiguous_origins.len(), 2);
    }

    #[test]
    fn item_records_and_utf8_leaf_witnesses_have_fixed_caps() {
        let mut census = ItemCensus::default();
        for _ in 0..MAX_ITEMS + 2 {
            ir_owner("generic", None, None, &[], &mut census);
        }
        assert_eq!(census.ir.len(), MAX_ITEMS);
        assert_eq!(census.ir_total, MAX_ITEMS + 2);
        let mut witness = String::new();
        append_witness(&mut witness, &"α".repeat(MAX_WITNESS));
        assert_eq!(witness.len(), MAX_WITNESS);
        append_witness(&mut witness, "later words");
        assert_eq!(witness.len(), MAX_WITNESS);
    }
}
