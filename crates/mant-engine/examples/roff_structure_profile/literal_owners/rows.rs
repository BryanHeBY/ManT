//! Reconstruct uncomplicated native rows with one lexical state per field.

use libmandoc_rs::{Node, NodeKind};

use super::{ScopeOccurrence, SourceContext, SourceDisplay, normalized_row, source_evidence};
use std::collections::BTreeMap;

pub(super) struct CellRow {
    pub(super) owner: Option<usize>,
    pub(super) text: String,
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
    use mant_codec::RoffWordEffect::{EmittedBreak, Unclassified, WordEndBreak};
    let mut rows = Vec::new();
    let mut start = 0;
    for end in 0..=nodes.len() {
        let next = nodes.get(end);
        let closes = next.is_none_or(|n| {
            matches!(n.macro_token.as_deref(), Some("br" | "Pp"))
                || n.kind == NodeKind::Block
                    && matches!(n.macro_token.as_deref(), Some("D1" | "Dl" | "Bd" | "Bl"))
        });
        if closes || next.is_some_and(|n| n.flags.no_fill && n.flags.line_start) {
            if start < end {
                let field = &nodes[start..end];
                if !field.iter().all(simple_inline) {
                    return None;
                }
                let (prefix, word) = source_evidence(field, origin, false, source, true);
                let word = word?;
                if !prefix.absent()
                    || word.has(Unclassified)
                    || word.has(WordEndBreak)
                    || word.has(EmittedBreak)
                {
                    return None;
                }
                let text = normalized_row(&word.text);
                if !text.is_empty() {
                    rows.push((field[0].id, text));
                }
            }
            start = end + usize::from(closes);
        }
    }
    Some(rows)
}

pub(super) fn display_rows(node: &Node, source: &SourceContext) -> Option<BTreeMap<u32, String>> {
    let body_index = node
        .children
        .iter()
        .position(|n| n.kind == NodeKind::Body)?;
    let mut rows = lexical_rows(&node.children[body_index].children, node, source)?;
    if matches!(node.macro_token.as_deref(), Some("D1" | "Dl"))
        && let Some((_, text)) = rows.last_mut()
    {
        // mdoc_macro.c places trailing delimiters after the short display BODY.
        for tail in &node.children[body_index + 1..] {
            if !tail.flags.delimiter_close || !simple_inline(tail) {
                return None;
            }
            text.push_str(&mant_codec::RoffWordEvidence::read(tail.text.as_deref()?).text);
        }
    }
    Some(rows.into_iter().collect())
}

pub(super) fn cell_plan(
    body: &Node,
    source: &SourceContext,
    owners: &[SourceDisplay],
    base: usize,
    path: &[ScopeOccurrence],
) -> Option<Vec<CellRow>> {
    let direct: BTreeMap<_, _> = owners
        .iter()
        .enumerate()
        .filter(|(_, o)| o.path == path)
        .map(|(i, o)| (o.id, base + i))
        .collect();
    let mut found = 0;
    let mut start = 0;
    let mut result = Vec::new();
    for (i, child) in body.children.iter().enumerate() {
        if let Some(&owner) = direct.get(&child.id) {
            found += 1;
            result.extend(
                lexical_rows(&body.children[start..i], body, source)?
                    .into_iter()
                    .map(|(_, text)| CellRow { owner: None, text }),
            );
            let rows = display_rows(child, source)?;
            if rows.is_empty() {
                return None;
            }
            result.extend(rows.into_values().map(|text| CellRow {
                owner: Some(owner),
                text,
            }));
            start = i + 1;
        } else if child.kind == NodeKind::Block && !simple_inline(child) {
            // Nested owners have separate geometry. An unmodelled neighboring
            // block could also supply equal ink, so abstain on this cell.
            return None;
        }
    }
    result.extend(
        lexical_rows(&body.children[start..], body, source)?
            .into_iter()
            .map(|(_, text)| CellRow { owner: None, text }),
    );
    (found == direct.len()).then_some(result)
}
