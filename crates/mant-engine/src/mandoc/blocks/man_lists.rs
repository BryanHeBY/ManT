//! Reconstructs source-proven lists expressed with man(7) tagged paragraphs.

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
pub(super) enum ManListState {
    None,
    Ordered {
        block: usize,
        marker: ManOrdinalMarker,
    },
}

impl ManListState {
    pub(super) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ManOrdinalMarker {
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

/// Parse only source-proven enumerator spellings used by man(7) `.IP`/`.TP`.
///
/// A bare resolved integer is ambiguous: option manuals routinely use the
/// same spelling for a real value domain. It is therefore accepted only when
/// the source line used roff's pre-increment register form. Punctuation is
/// retained as a sequence style so `1.` followed by `2)` cannot accidentally
/// merge across unrelated tagged paragraphs.
pub(super) fn ordinal_marker(
    item: &DefinitionItem,
    uses_incrementing_register: bool,
) -> Option<ManOrdinalMarker> {
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
    Some(ManOrdinalMarker { value, style })
}

/// Convert a source-proven man enumerator into an ordered list, appending it
/// to an adjacent sequence when the spelling and numeric progression agree.
///
/// Punctuated integers and source-level incrementing registers carry enough
/// evidence even when a list contains only one item.  Requiring a second item
/// used to leak singleton footnote labels such as `1.` into the semantic entry
/// index.  Bare literal integers remain excluded by [`ordinal_marker`].
pub(super) fn append_ordered(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<SourceSpan>,
    marker: ManOrdinalMarker,
    state: &mut ManListState,
) {
    match *state {
        ManListState::Ordered {
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
                *state = ManListState::None;
                append_new_ordered(
                    output,
                    item,
                    indent_columns,
                    paragraph_distance,
                    source,
                    marker,
                    state,
                );
                return;
            };
            *compact = *compact && paragraph_distance == 0;
            items.push(list_item_from_definition(item, indent_columns, source));
            *state = ManListState::Ordered { block, marker };
        }
        ManListState::None | ManListState::Ordered { .. } => {
            append_new_ordered(
                output,
                item,
                indent_columns,
                paragraph_distance,
                source,
                marker,
                state,
            );
        }
    }
}

fn append_new_ordered(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<SourceSpan>,
    marker: ManOrdinalMarker,
    state: &mut ManListState,
) {
    let block = output.len();
    output.push(Block::List {
        kind: ListKind::Ordered,
        start: Some(marker.value),
        compact: paragraph_distance == 0,
        items: vec![list_item_from_definition(item, indent_columns, source)],
        layout: layout_with_spacing(indent_columns, paragraph_distance),
        source,
    });
    *state = ManListState::Ordered { block, marker };
}

/// Attach a transparent relative-indent scope to the current `.IP` item.
///
/// AsciDoc-generated man pages commonly put a reference URI in an `.RS/.RE`
/// scope immediately after each numbered `.IP`.  Libmandoc correctly exposes
/// that scope as a sibling of the `.IP`, but it remains content of the same
/// visible item and must not break ordinal sequence recognition.  Proven list
/// items use coordinates relative to the list container.
pub(super) fn append_relative_continuation(
    output: &mut [Block],
    nested: &mut Vec<Block>,
    indent_columns: u16,
    state: ManListState,
) -> bool {
    let origin = indent_columns.saturating_add(MAN_DEFINITION_BODY_INDENT);
    match state {
        ManListState::Ordered { block, .. } => {
            let Some(Block::List {
                kind: ListKind::Ordered,
                items,
                ..
            }) = output.get_mut(block)
            else {
                return false;
            };
            let Some(item) = items.last_mut() else {
                return false;
            };
            make_relative(nested, origin);
            item.blocks.append(nested);
            true
        }
        ManListState::None => false,
    }
}

fn make_relative(blocks: &mut [Block], origin: u16) {
    for block in blocks {
        if let Some(layout) = block_layout_mut(block) {
            layout.indent_columns = layout.indent_columns.saturating_sub(origin);
        }
    }
}

/// Remove an `.IP`/`.TP` mark from visible content while conserving any target
/// it owned and making item indentation relative to the new list container.
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
            Inline::Anchor { id, .. } => output.push(id.to_string()),
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
