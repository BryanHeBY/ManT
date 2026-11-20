//! Row witnesses for short, source-proved no-fill display runs.

use super::{
    DisplayRow, Node, NodeKind, Origin, RoffWordEffect, RoffWordEvidence, SourceContext,
    TermPayload, record_words, source_evidence,
};
use libmandoc_rs::DisplayKind;

pub(super) fn record(
    display: &Node,
    body: &Node,
    source: &SourceContext,
    payload: &mut TermPayload,
) -> bool {
    if !matches!(
        display.display_kind,
        Some(DisplayKind::Literal | DisplayKind::Unfilled)
    ) {
        return false;
    }
    let Some(words) = body
        .children
        .iter()
        .map(|node| match node.kind {
            NodeKind::Text => Some(RoffWordEvidence::read(
                node.text.as_deref().unwrap_or_default(),
            )),
            NodeKind::Element
                if node.macro_token.as_deref() == Some("No")
                    && node
                        .children
                        .iter()
                        .all(|child| child.kind == NodeKind::Text) =>
            {
                source_evidence(std::slice::from_ref(node), node, false, source, true).1
            }
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };

    // print_mdoc_node() honors a pending \c at node entry. A tag field
    // can still retain a later macro word on its current physical line
    // (termp_it_pre's NOBREAK/BRTRSP). Prove relative hard rows for raw
    // source rows; leave an independent macro-word boundary ungraded.
    payload.displays += 1;
    let mut row = DisplayRow {
        display: payload.displays,
        within: 0,
    };
    let mut start = 0;
    let mut continues = false;
    let mut proved_rows = true;
    for (index, (node, word)) in body.children.iter().zip(&words).enumerate() {
        if index > 0 && node.flags.line_start && !continues && node.kind != NodeKind::Text {
            proved_rows = false;
        }
        continues = word.has(RoffWordEffect::Continues);
    }
    let mut continues = false;
    for (index, (node, word)) in body.children.iter().zip(&words).enumerate() {
        if index > start && node.flags.line_start && !continues {
            record_words(
                &body.children[start..index],
                source,
                Origin::Display,
                proved_rows.then_some(row),
                payload,
            );
            start = index;
            row.within += 1;
        }
        continues = word.has(RoffWordEffect::Continues);
    }
    record_words(
        &body.children[start..],
        source,
        Origin::Display,
        proved_rows.then_some(row),
        payload,
    );
    payload.partial |= !proved_rows;
    true
}
