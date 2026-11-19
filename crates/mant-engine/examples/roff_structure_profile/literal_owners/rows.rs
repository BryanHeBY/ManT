//! Reconstruct uncomplicated native rows with one lexical state per field.

use libmandoc_rs::{Node, NodeKind};

use super::{ScopeOccurrence, SourceContext, SourceDisplay, normalized_row, source_evidence};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct CellRow {
    pub(super) owner: Option<usize>,
    pub(super) text: String,
}

#[derive(Default)]
pub(super) struct CellPlan {
    pub(super) rows: Vec<CellRow>,
    pub(super) verified: BTreeSet<usize>,
    pub(super) complete: bool,
}

fn simple_inline(node: &Node) -> bool {
    if node.flags.synopsis_pretty {
        return false;
    }
    if node.kind == NodeKind::Text {
        let mut source = node.text.as_deref().unwrap_or_default();
        while let Some((_, rest)) = source.split_once('\\') {
            if let Some(tail) = rest.strip_prefix(['&', '-']) {
                source = tail;
            } else if let Some((hex, tail)) =
                rest.strip_prefix("[u").and_then(|s| s.split_once(']'))
                && !hex.is_empty()
                && hex.bytes().all(|b| b.is_ascii_hexdigit())
            {
                source = tail;
            } else {
                return false;
            }
        }
        return true;
    }
    // These CVS mdoc_term.c handlers select a font, emit known operands
    // and delimiters, or glue words. Other operations need independent proof.
    matches!(
        node.kind,
        NodeKind::Element | NodeKind::Block | NodeKind::Head | NodeKind::Body
    ) && matches!(
        node.macro_token.as_deref(),
        Some(
            "Em" | "Sy"
                | "Li"
                | "Ar"
                | "Cm"
                | "Ic"
                | "Pa"
                | "Ev"
                | "Va"
                | "Dv"
                | "Er"
                | "Fl"
                | "Fn"
                | "Xr"
                | "Lk"
                | "Bq"
                | "Op"
                | "No"
                | "Ns"
        )
    ) && node.children.iter().all(simple_inline)
}

fn lexical_rows(
    nodes: &[Node],
    origin: &Node,
    source: &SourceContext,
) -> Option<Vec<(u32, String)>> {
    let mut rows = Vec::new();
    let mut start = 0;
    for end in 0..=nodes.len() {
        let next = nodes.get(end);
        let closes = next.is_none_or(|n| {
            matches!(n.macro_token.as_deref(), Some("br" | "Pp" | "sp"))
                || n.kind == NodeKind::Block
                    && matches!(n.macro_token.as_deref(), Some("D1" | "Dl" | "Bd" | "Bl"))
                // termp_sm_pre changes the next word's spacing only.
                || end + 1 == nodes.len() && n.macro_token.as_deref() == Some("Sm")
        });
        if closes || next.is_some_and(starts_source_row) {
            if start < end {
                let field = &nodes[start..end];
                if !field.iter().all(simple_inline) {
                    return None;
                }
                let text = lexical_row(field, origin, source)?;
                if !text.is_empty() {
                    rows.push((field[0].id, text));
                }
            }
            start = end + usize::from(closes);
        }
    }
    Some(rows)
}

fn starts_source_row(node: &Node) -> bool {
    // mdoc_term.c::print_mdoc_node ends a row for a leading ASCII space
    // in filled TEXT as well as for NODE_LINE in no-fill. lexical_row
    // rejects a continuation escape before either inferred boundary.
    node.flags.line_start
        && (node.flags.no_fill
            || node.kind == NodeKind::Text
                && node
                    .decoder_text()
                    .is_some_and(|text| text.starts_with(' ')))
}

fn lexical_row(nodes: &[Node], origin: &Node, source: &SourceContext) -> Option<String> {
    use mant_codec::RoffWordEffect::{EmittedBreak, Unclassified, WordEndBreak};
    let (prefix, word) = source_evidence(nodes, origin, false, source, true);
    let word = word?;
    (prefix.absent()
        && !word.has(Unclassified)
        && !word.has(WordEndBreak)
        && !word.has(EmittedBreak))
    .then(|| normalized_row(&word.text))
}

pub(super) fn display_rows(node: &Node, source: &SourceContext) -> Option<BTreeMap<u32, String>> {
    if matches!(node.macro_token.as_deref(), Some("D1" | "Dl")) {
        // Keep one lexical state across the complete short-display field,
        // including sibling delimiter TEXT and invisible BODY words.
        if !node.children.iter().all(|part| match part.kind {
            NodeKind::Head | NodeKind::Body => part.children.iter().all(simple_inline),
            _ => part.kind == NodeKind::Text && simple_inline(part),
        }) {
            return None;
        }
        let text = lexical_row(&node.children, node, source)?;
        return Some(
            (!text.is_empty())
                .then_some((node.id, text))
                .into_iter()
                .collect(),
        );
    }
    let body = node.children.iter().find(|n| n.kind == NodeKind::Body)?;
    Some(
        lexical_rows(&body.children, node, source)?
            .into_iter()
            .collect(),
    )
}

pub(super) fn cell_plan(
    body: &Node,
    source: &SourceContext,
    owners: &[SourceDisplay],
    base: usize,
    path: &[ScopeOccurrence],
) -> CellPlan {
    let direct: BTreeMap<_, _> = owners
        .iter()
        .enumerate()
        .filter(|(_, o)| o.path == path)
        .map(|(i, o)| (o.id, base + i))
        .collect();
    let mut plan = CellPlan::default();
    plan.complete = extend_cell_plan(body, source, &direct, &mut plan).is_some();
    plan
}

fn extend_cell_plan(
    body: &Node,
    source: &SourceContext,
    direct: &BTreeMap<u32, usize>,
    plan: &mut CellPlan,
) -> Option<()> {
    let mut start = 0;
    for (i, child) in body.children.iter().enumerate() {
        if let Some(&owner) = direct.get(&child.id) {
            plan.rows.extend(
                lexical_rows(&body.children[start..i], body, source)?
                    .into_iter()
                    .map(|(_, text)| CellRow { owner: None, text }),
            );
            let rows = display_rows(child, source)?;
            if rows.is_empty() {
                return None;
            }
            plan.rows.extend(rows.into_values().map(|text| CellRow {
                owner: Some(owner),
                text,
            }));
            plan.verified.insert(owner);
            start = i + 1;
        } else if child.kind == NodeKind::Block && !simple_inline(child) {
            // Nested owners have separate geometry. An unmodelled neighboring
            // block could also supply equal ink, so abstain on this cell.
            return None;
        }
    }
    plan.rows.extend(
        lexical_rows(&body.children[start..], body, source)?
            .into_iter()
            .map(|(_, text)| CellRow { owner: None, text }),
    );
    (plan.verified.len() == direct.len()).then_some(())
}
