//! Reconstructs source-proven lists expressed with man(7) tagged paragraphs.

use mant_ir::{Block, DefinitionItem, ListItem, ListKind, SourceSpan};

use crate::block::rebase_roots;
use crate::mandoc::{
    inline::plain_text,
    layout::{layout, layout_with_spacing},
    targets,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::blocks) struct DefinitionLocation {
    pub(in crate::mandoc::blocks) block: usize,
    pub(in crate::mandoc::blocks) item: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::blocks) enum ManListState {
    None,
    Ordered {
        block: usize,
        marker: ManOrdinalMarker,
    },
}

impl ManListState {
    pub(in crate::mandoc::blocks) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::blocks) struct ManOrdinalMarker {
    value: u64,
    style: IpOrdinalStyle,
}

impl ManOrdinalMarker {
    pub(in crate::mandoc::blocks) const fn value(self) -> u64 {
        self.value
    }
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
pub(in crate::mandoc::blocks) fn ordinal_marker(
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

/// Recognize an entire mdoc tag list as one explicit ordinal sequence.
///
/// Unlike man(7) tagged paragraphs, `.Bl -tag` is explicit evidence for a
/// definition list. Override it only when at least two described items form a
/// complete, consecutive sequence with one punctuation style. This leaves
/// singleton numeric terms and value domains untouched.
pub(in crate::mandoc::blocks) fn ordinal_sequence(
    items: &[DefinitionItem],
) -> Option<ManOrdinalMarker> {
    if items.len() < 2 {
        return None;
    }
    let first = ordinal_marker(&items[0], false)?;
    let mut previous = first;
    for item in &items[1..] {
        let marker = ordinal_marker(item, false)?;
        if marker.style != previous.style || previous.value.checked_add(1) != Some(marker.value) {
            return None;
        }
        previous = marker;
    }
    Some(first)
}

/// Convert a source-proven man enumerator into an ordered list, appending it
/// to an adjacent sequence when the spelling and numeric progression agree.
///
/// Punctuated integers and source-level incrementing registers carry enough
/// evidence even when a list contains only one item.  Requiring a second item
/// used to leak singleton footnote labels such as `1.` into the semantic entry
/// index.  Bare literal integers remain excluded by [`ordinal_marker`].
pub(in crate::mandoc::blocks) fn append_ordered(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: crate::mandoc::layout::SourceIndent,
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
                kind: ListKind::Ordered { .. },
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
            items.push(spaced_man_list_item(
                item,
                ordinal_width(marker.value),
                source,
                paragraph_distance,
            ));
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
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: u16,
    source: Option<SourceSpan>,
    marker: ManOrdinalMarker,
    state: &mut ManListState,
) {
    let block = output.len();
    output.push(Block::List {
        kind: ListKind::Ordered {
            start: Some(marker.value),
        },
        compact: paragraph_distance == 0,
        items: vec![spaced_man_list_item(
            item,
            ordinal_width(marker.value),
            source,
            paragraph_distance,
        )],
        layout: layout_with_spacing(indent_columns, 0),
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
pub(in crate::mandoc::blocks) fn append_relative_continuation(
    output: &mut [Block],
    nested: &mut Vec<Block>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    state: ManListState,
) -> bool {
    match state {
        ManListState::Ordered { block, marker } => {
            let origin = indent_columns
                .relative_columns()
                .saturating_add(ordinal_width(marker.value));
            let Some(Block::List {
                kind: ListKind::Ordered { .. },
                items,
                ..
            }) = output.get_mut(block)
            else {
                return false;
            };
            let Some(item) = items.last_mut() else {
                return false;
            };
            rebase_roots(nested, 0, origin);
            item.blocks.append(nested);
            true
        }
        ManListState::None => false,
    }
}

fn ordinal_width(value: u64) -> i32 {
    mant_protocol::geometry::coordinate(mant_protocol::geometry::text_width(&format!("{value}. ")))
}

/// The tagged paragraph's leading boundary precedes its marker, not its
/// first body block. Store it once on the item, regardless of list grouping.
pub(in crate::mandoc::blocks) fn spaced_man_list_item(
    item: DefinitionItem,
    marker_width: i32,
    source: Option<SourceSpan>,
    paragraph_distance: u16,
) -> ListItem {
    let mut item = list_item_from_definition(item, marker_width, source);
    item.layout.spacing_before_lines = Some(paragraph_distance);
    item
}

/// Remove an `.IP`/`.TP` mark from visible content while conserving any target
/// it owned and making item indentation relative to the new list container.
pub(in crate::mandoc::blocks) fn list_item_from_definition(
    item: DefinitionItem,
    marker_width: i32,
    source: Option<SourceSpan>,
) -> ListItem {
    let DefinitionItem {
        source: item_source,
        terms,
        mut description,
        layout: definition_layout,
        ..
    } = item;
    // The source body column survives semantic conversion. Only these roots
    // move from the definition body origin to the list marker's body origin.
    rebase_roots(
        &mut description,
        definition_layout.body_indent_columns,
        marker_width,
    );
    let mut anchors = Vec::new();
    for term in &terms {
        targets::inline_anchor_ids(term, &mut anchors);
    }
    targets::attach_targets(
        &mut description,
        anchors,
        layout(crate::mandoc::layout::SourceIndent::default()),
        source,
    );
    ListItem {
        layout: mant_ir::ListItemLayout::default(),
        source: item_source,
        entry: None,
        blocks: description,
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem, Inline, LayoutHint, ListKind};

    fn definition(term: &str, description: &str) -> DefinitionItem {
        DefinitionItem {
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
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

    #[test]
    fn recognizes_only_complete_punctuated_mdoc_sequences() {
        let sequence = [
            definition("1.", "one"),
            definition("2.", "two"),
            definition("3.", "three"),
        ];
        assert_eq!(
            super::ordinal_sequence(&sequence).map(super::ManOrdinalMarker::value),
            Some(1)
        );

        assert!(super::ordinal_sequence(&sequence[..1]).is_none());
        assert!(
            super::ordinal_sequence(&[definition("1.", "one"), definition("3.", "three")])
                .is_none()
        );
        assert!(
            super::ordinal_sequence(&[definition("1.", "one"), definition("2)", "two")]).is_none()
        );
        assert!(
            super::ordinal_sequence(&[definition("0", "off"), definition("1", "on")]).is_none()
        );
    }

    #[test]
    fn ordered_conversion_conserves_a_native_term_target() {
        let mut item = definition("1.", "item");
        item.terms[0].insert(0, Inline::anchor("native-target"));
        let marker = super::ordinal_marker(&item, false).expect("punctuated ordinal");
        let mut output = Vec::new();
        let mut state = super::ManListState::None;

        super::append_ordered(
            &mut output,
            item,
            crate::mandoc::layout::SourceIndent::default(),
            1,
            None,
            marker,
            &mut state,
        );

        let [
            Block::List {
                kind: ListKind::Ordered { .. },
                items,
                ..
            },
        ] = output.as_slice()
        else {
            panic!("ordered list");
        };
        assert!(matches!(
            items[0].blocks[0],
            Block::Paragraph { ref children, .. }
                if matches!(children.first(), Some(Inline::Anchor { id, .. }) if id == "native-target")
        ));
    }
}
