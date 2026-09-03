//! Reconstructs source-proven lists expressed with man(7) `.IP` paragraphs.

use mant_ir::{Block, DefinitionItem, Inline, ListItem, ListKind, SourceSpan};

use super::super::{
    inline::plain_text,
    layout::{layout, layout_with_spacing},
    targets,
};
use crate::block::block_layout_mut;

pub(super) const MAN_DEFINITION_BODY_INDENT: u16 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DefinitionLocation {
    pub(super) block: usize,
    pub(super) item: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ManIpListState {
    None,
    Candidate {
        location: DefinitionLocation,
        marker: IpOrdinalMarker,
        source: Option<SourceSpan>,
    },
    Ordered {
        block: usize,
        marker: IpOrdinalMarker,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct IpOrdinalMarker {
    value: u64,
    style: IpOrdinalStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IpOrdinalStyle {
    Period,
    ClosingParenthesis,
    Parenthesized,
    Bracketed,
    IncrementingRegister,
}

/// Parse only source-proven enumerator spellings used by man(7) `.IP`.
///
/// A bare resolved integer is ambiguous: option manuals routinely use the
/// same spelling for a real value domain. It is therefore accepted only when
/// the source line used roff's pre-increment register form. Punctuation is
/// retained as a sequence style so `1.` followed by `2)` cannot accidentally
/// merge across unrelated tagged paragraphs.
pub(super) fn ordinal_marker(
    item: &DefinitionItem,
    uses_incrementing_register: bool,
) -> Option<IpOrdinalMarker> {
    if item.description.is_empty() {
        return None;
    }
    let [term] = item.terms.as_slice() else {
        return None;
    };
    let text = plain_text(term);
    let text = text.trim();
    let (digits, style) = if let Some(digits) = text.strip_suffix('.') {
        (digits, IpOrdinalStyle::Period)
    } else if let Some(digits) = text.strip_suffix(')') {
        if let Some(digits) = digits.strip_prefix('(') {
            (digits, IpOrdinalStyle::Parenthesized)
        } else {
            (digits, IpOrdinalStyle::ClosingParenthesis)
        }
    } else if let Some(digits) = text
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        (digits, IpOrdinalStyle::Bracketed)
    } else if uses_incrementing_register {
        (text, IpOrdinalStyle::IncrementingRegister)
    } else {
        return None;
    };
    let value = digits.parse().ok()?;
    Some(IpOrdinalMarker { value, style })
}

/// Convert a proven sequence of adjacent `.IP` enumerators into one ordered
/// list, or append to an already proven sequence.
///
/// The first candidate remains a definition until the next consecutive,
/// same-style marker arrives. This is important because `.IP` itself carries
/// no list semantics: the same macro is also the portable definition-list
/// primitive. Semantic entry discovery runs only after this pass completes,
/// so the temporary representation never escapes the lowering boundary.
pub(super) fn append_ordered(
    output: &mut Vec<Block>,
    item: &mut Option<DefinitionItem>,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<SourceSpan>,
    marker: IpOrdinalMarker,
    state: &mut ManIpListState,
) -> bool {
    match *state {
        ManIpListState::Ordered {
            block,
            marker: previous,
        } if previous.style == marker.style
            && previous.value.checked_add(1) == Some(marker.value)
            && block == output.len().saturating_sub(1) =>
        {
            let Some(Block::List {
                kind: ListKind::Ordered,
                compact,
                items,
                ..
            }) = output.get_mut(block)
            else {
                *state = ManIpListState::None;
                return false;
            };
            *compact = *compact && paragraph_distance == 0;
            items.push(list_item_from_definition(
                item.take().expect("ordered item exists"),
                indent_columns,
                source,
            ));
            *state = ManIpListState::Ordered { block, marker };
            true
        }
        ManIpListState::Candidate {
            location,
            marker: previous,
            source: previous_source,
        } if previous.style == marker.style
            && previous.value.checked_add(1) == Some(marker.value)
            && location.block == output.len().saturating_sub(1) =>
        {
            let Some(Block::DefinitionList {
                items,
                layout: definition_layout,
                source: definition_source,
                ..
            }) = output.get_mut(location.block)
            else {
                *state = ManIpListState::None;
                return false;
            };
            if location.item != items.len().saturating_sub(1) {
                *state = ManIpListState::None;
                return false;
            }
            let Some(previous_item) = items.pop() else {
                *state = ManIpListState::None;
                return false;
            };
            let first_spacing = previous_item.spacing_before_lines.unwrap_or(0);
            let list_source = previous_source.or(*definition_source);
            let remove_definition = items.is_empty();
            let spacing_before_lines = if remove_definition {
                definition_layout.spacing_before_lines
            } else {
                first_spacing
            };
            if remove_definition {
                output.pop();
            }
            let block = output.len();
            output.push(Block::List {
                kind: ListKind::Ordered,
                start: Some(previous.value),
                compact: paragraph_distance == 0,
                items: vec![
                    list_item_from_definition(previous_item, indent_columns, list_source),
                    list_item_from_definition(
                        item.take().expect("ordered item exists"),
                        indent_columns,
                        source,
                    ),
                ],
                layout: layout_with_spacing(indent_columns, spacing_before_lines),
                source: list_source,
            });
            *state = ManIpListState::Ordered { block, marker };
            true
        }
        ManIpListState::None
        | ManIpListState::Candidate { .. }
        | ManIpListState::Ordered { .. } => {
            *state = ManIpListState::None;
            false
        }
    }
}

/// Remove an `.IP` mark from visible content while conserving any target it
/// owned and making item indentation relative to the new list container.
pub(super) fn list_item_from_definition(
    item: DefinitionItem,
    indent_columns: u16,
    source: Option<SourceSpan>,
) -> ListItem {
    let DefinitionItem {
        terms,
        mut description,
        ..
    } = item;
    for block in &mut description {
        if let Some(layout) = block_layout_mut(block) {
            layout.indent_columns = layout
                .indent_columns
                .saturating_sub(indent_columns.saturating_add(MAN_DEFINITION_BODY_INDENT));
        }
    }
    let mut anchors = Vec::new();
    for term in &terms {
        collect_inline_anchors(term, &mut anchors);
    }
    targets::attach_targets(&mut description, anchors, layout(0), source);
    ListItem {
        blocks: description,
    }
}

fn collect_inline_anchors(nodes: &[Inline], output: &mut Vec<String>) {
    for node in nodes {
        match node {
            Inline::Anchor { id } => output.push(id.to_string()),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect_inline_anchors(children, output),
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem, Inline, LayoutHint};

    fn definition(term: &str, description: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            inline_term: false,
            terms: vec![vec![Inline::Text {
                value: term.to_owned(),
            }]],
            description: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: description.to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            spacing_before_lines: None,
        }
    }

    #[test]
    fn recognizes_only_unambiguous_ordinal_spellings() {
        for marker in ["1.", "2)", "(3)", "[4]"] {
            assert!(super::ordinal_marker(&definition(marker, "item"), false).is_some());
        }
        assert!(super::ordinal_marker(&definition("1", "item"), true).is_some());
        for value in ["1", "2.2", "v1.", "1.2."] {
            assert!(super::ordinal_marker(&definition(value, "value"), false).is_none());
        }
        let mut empty = definition("1.", "");
        empty.description.clear();
        assert!(super::ordinal_marker(&empty, false).is_none());
    }
}
